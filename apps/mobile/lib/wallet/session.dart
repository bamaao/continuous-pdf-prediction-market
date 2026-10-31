import 'dart:convert';

import 'package:flutter/foundation.dart';

import '../api/desk_api.dart';
import '../config.dart';
import '../mwa_channel.dart';
import '../solana/codec.dart';
import '../solana/legacy_tx.dart';
import '../solana/rules.dart';
import '../solana/rpc.dart';
import 'nonce_book.dart';

class WalletSession extends ChangeNotifier {
  WalletSession({MwaChannel? channel, SolanaRpc? rpc, DeskApi? api})
    : _channel = channel ?? MwaChannel(),
      rpc = rpc ?? SolanaRpc(AppConfig.rpc),
      api = api ?? DeskApi(AppConfig.api);

  final MwaChannel _channel;
  final SolanaRpc rpc;
  final DeskApi api;
  NonceBook? _nonceBook;
  Future<NonceBook> Function()? _openBook;

  void useNonceBook(Future<NonceBook> Function() open) => _openBook = open;

  Future<NonceBook> _book() async => _nonceBook ??= await (_openBook ?? NonceBook.prefs)();

  Uint8List? publicKey;
  String? address;
  bool busy = false;
  String note = '';

  bool get connected => publicKey != null;

  Future<void> connect() async {
    await _work(() async {
      final encoded = await _channel.authorize(cluster: AppConfig.cluster, origin: AppConfig.identity);
      publicKey = base64Decode(encoded);
      if (publicKey!.length != 32) throw StateError('钱包公钥长度不对');
      address = base58Encode(publicKey!);
      note = '已连接';
    });
  }

  Future<void> disconnect() async {
    await _channel.disconnect();
    publicKey = null;
    address = null;
    note = '';
    notifyListeners();
  }

  Future<NonceChoice> chooseNonce(String market) async {
    final owner = address;
    if (owner == null) throw StateError('先连接钱包');
    final last = await rpc.fillNonceLast(owner, market);
    final stored = await (await _book()).stored(owner, market);
    final choice = planNonce(chainLast: last, stored: stored);
    if (choice.replay) await (await _book()).save(owner, market, choice.nonce);
    return choice;
  }

  Future<void> rememberNonce(String market, int next) async {
    final owner = address;
    if (owner == null) return;
    await (await _book()).save(owner, market, next);
  }

  Future<String> submit(Map<String, Object?> body) async {
    final ixs = await api.compose(body);
    return submitIxs(ixs);
  }

  Future<String> submitIxs(List<ComposeIx> ixs) async {
    final payer = publicKey;
    if (payer == null) throw StateError('先连接钱包');
    _checkIxs(ixs, payer);
    var last = '';
    for (final ix in ixs) {
      last = await _submitOne(payer, [ix]);
    }
    return last;
  }

  void _checkIxs(List<ComposeIx> ixs, Uint8List payer) {
    final mine = base58Encode(payer);
    for (final ix in ixs) {
      if (!programAllowed(ix.programId)) {
        throw StateError('指令程序不在允许名单里 ${ix.programId}');
      }
      for (final key in ix.keys) {
        if (key.signer && key.pubkey != mine) {
          throw StateError('指令要求其他账户签名');
        }
      }
    }
  }

  Future<String> _submitOne(Uint8List payer, List<ComposeIx> ixs) async {
    final hash = base58Decode(await rpc.latestBlockhash());
    final raw = compileLegacy(
      feePayer: payer,
      recentBlockhash: hash,
      instructions: [heapFrameIx(), computeLimitIx(), ...ixs.map((ix) => ix.toCompiled())],
    );
    final signed = await _channel.signTransactions([base64Encode(raw)]);
    final sig = await rpc.send(signed.first);
    await rpc.confirm(sig);
    return sig;
  }

  Future<void> _work(Future<void> Function() action) async {
    busy = true;
    note = '';
    notifyListeners();
    try {
      await action();
    } catch (e) {
      note = '$e';
      rethrow;
    } finally {
      busy = false;
      notifyListeners();
    }
  }
}
