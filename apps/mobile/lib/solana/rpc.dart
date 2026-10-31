import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;

import 'codec.dart';
import 'pda.dart';
import 'rules.dart';

class SolanaRpc {
  SolanaRpc(this.url, {bool? release}) : _release = release ?? kReleaseMode;

  final String url;
  final bool _release;

  Future<String> latestBlockhash() async {
    final value = await _call('getLatestBlockhash', [
      {'commitment': 'confirmed'},
    ]);
    final hash = (value as Map)['value']['blockhash'];
    if (hash is! String || hash.isEmpty) throw StateError('RPC 没有返回 blockhash');
    return hash;
  }

  Future<String> send(String signedTxB64) async {
    final sig = await _call('sendTransaction', [
      signedTxB64,
      {'encoding': 'base64', 'skipPreflight': true},
    ]);
    if (sig is! String || sig.isEmpty) throw StateError('RPC 没有返回签名');
    return sig;
  }

  Future<void> confirm(String signature) async {
    for (var i = 0; i < 40; i++) {
      final value = await _call('getSignatureStatuses', [
        [signature],
        {'searchTransactionHistory': false},
      ]);
      final row = ((value as Map)['value'] as List).firstOrNull;
      if (row is Map) {
        if (row['err'] != null) {
          final logs = await _logs(signature);
          final detail = logs.isEmpty ? '${row['err']}' : logs;
          throw StateError('交易失败 $detail');
        }
        final status = row['confirmationStatus'];
        if (status == 'confirmed' || status == 'finalized') return;
      }
      await Future<void>.delayed(const Duration(milliseconds: 500));
    }
    throw StateError('交易仍在确认 $signature');
  }

  Future<Uint8List?> account(String pubkey) async {
    final value = await _call('getAccountInfo', [
      pubkey,
      {'encoding': 'base64'},
    ]);
    if (value is! Map) return null;
    final inner = value['value'];
    if (inner is! Map) return null;
    final data = inner['data'];
    if (data is List && data.isNotEmpty && data.first is String) {
      return base64Decode(data.first as String);
    }
    return null;
  }

  Future<int> fillNonceLast(String owner, String market) async {
    final pda = base58Encode(noncePda(base58Decode(owner), base58Decode(market)));
    final data = await account(pda);
    if (data == null) return 0;
    if (data.length < 16) throw StateError('nonce 账户数据太短');
    final view = ByteData.sublistView(data);
    final lo = view.getUint32(8, Endian.little);
    final hi = view.getUint32(12, Endian.little);
    return lo + (hi << 32);
  }

  Future<bool> gridIsSealed(String market, int n) async {
    final pda = base58Encode(gridPda(base58Decode(market)));
    final data = await account(pda);
    if (data == null) return false;
    return gridSealed(data, n);
  }

  Future<String> _logs(String signature) async {
    try {
      final value = await _call('getTransaction', [
        signature,
        {'encoding': 'json', 'commitment': 'confirmed', 'maxSupportedTransactionVersion': 0},
      ]);
      if (value is! Map) return '';
      final meta = value['meta'];
      if (meta is! Map) return '';
      final lines = meta['logMessages'];
      if (lines is! List) return '';
      final picked = lines.map((line) => '$line').where((line) => RegExp('failed|Error|error', caseSensitive: false).hasMatch(line)).take(6);
      return picked.join(' · ');
    } catch (_) {
      return '';
    }
  }

  Future<Object?> _call(String method, List<Object?> params) async {
    ensureTransport(url, release: _release);
    final response = await http.post(
      Uri.parse(url),
      headers: {'content-type': 'application/json'},
      body: jsonEncode({'jsonrpc': '2.0', 'id': 1, 'method': method, 'params': params}),
    );
    if (response.statusCode != 200) {
      throw StateError('RPC ${response.statusCode} ${response.body}');
    }
    final body = jsonDecode(response.body) as Map<String, dynamic>;
    if (body['error'] != null) throw StateError('RPC ${body['error']}');
    return body['result'];
  }
}
