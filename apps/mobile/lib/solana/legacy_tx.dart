import 'dart:typed_data';

import 'codec.dart';

class AccountMeta {
  AccountMeta(this.pubkey, {required this.signer, required this.writable});

  final Uint8List pubkey;
  final bool signer;
  final bool writable;
}

class CompiledIx {
  CompiledIx({required this.programId, required this.keys, required this.data});

  final Uint8List programId;
  final List<AccountMeta> keys;
  final Uint8List data;
}

class _Meta {
  _Meta({required this.signer, required this.writable});

  bool signer;
  bool writable;
}

Uint8List u32le(int value) {
  final data = ByteData(4)..setUint32(0, value, Endian.little);
  return data.buffer.asUint8List();
}

CompiledIx heapFrameIx() {
  return CompiledIx(
    programId: base58Decode('ComputeBudget111111111111111111111111111111'),
    keys: const [],
    data: Uint8List.fromList(<int>[1, ...u32le(256 * 1024)]),
  );
}

CompiledIx computeLimitIx() {
  return CompiledIx(
    programId: base58Decode('ComputeBudget111111111111111111111111111111'),
    keys: const [],
    data: Uint8List.fromList(<int>[2, ...u32le(1400000)]),
  );
}

String _key(Uint8List pubkey) => base58Encode(pubkey);

/// Legacy transaction with zeroed signatures, the bytes Mobile Wallet Adapter signs.
Uint8List compileLegacy({
  required Uint8List feePayer,
  required Uint8List recentBlockhash,
  required List<CompiledIx> instructions,
}) {
  if (feePayer.length != 32) throw ArgumentError('fee payer must be 32 bytes');
  if (recentBlockhash.length != 32) throw ArgumentError('blockhash must be 32 bytes');
  final order = <String>[];
  final meta = <String, _Meta>{};
  void touch(Uint8List pubkey, {required bool signer, required bool writable}) {
    final id = _key(pubkey);
    final existing = meta[id];
    if (existing == null) {
      order.add(id);
      meta[id] = _Meta(signer: signer, writable: writable);
      return;
    }
    existing.signer = existing.signer || signer;
    existing.writable = existing.writable || writable;
  }

  touch(feePayer, signer: true, writable: true);
  for (final ix in instructions) {
    touch(ix.programId, signer: false, writable: false);
    for (final key in ix.keys) {
      touch(key.pubkey, signer: key.signer, writable: key.writable);
    }
  }

  final writableSigners = <String>[];
  final readonlySigners = <String>[];
  final writable = <String>[];
  final readonly = <String>[];
  for (final id in order) {
    final item = meta[id]!;
    if (item.signer && item.writable) {
      writableSigners.add(id);
    } else if (item.signer) {
      readonlySigners.add(id);
    } else if (item.writable) {
      writable.add(id);
    } else {
      readonly.add(id);
    }
  }
  final accounts = <String>[...writableSigners, ...readonlySigners, ...writable, ...readonly];
  final indexOf = <String, int>{for (var i = 0; i < accounts.length; i++) accounts[i]: i};

  final message = BytesBuilder(copy: false);
  message.addByte(writableSigners.length + readonlySigners.length);
  message.addByte(readonlySigners.length);
  message.addByte(readonly.length);
  writeShortvec(message, accounts.length);
  for (final id in accounts) {
    message.add(base58Decode(id));
  }
  message.add(recentBlockhash);
  writeShortvec(message, instructions.length);
  for (final ix in instructions) {
    message.addByte(indexOf[_key(ix.programId)]!);
    writeShortvec(message, ix.keys.length);
    for (final key in ix.keys) {
      message.addByte(indexOf[_key(key.pubkey)]!);
    }
    writeShortvec(message, ix.data.length);
    message.add(ix.data);
  }

  final required = writableSigners.length + readonlySigners.length;
  final tx = BytesBuilder(copy: false);
  writeShortvec(tx, required);
  tx.add(Uint8List(required * 64));
  tx.add(message.toBytes());
  return tx.toBytes();
}
