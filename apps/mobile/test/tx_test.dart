import 'dart:typed_data';

import 'package:continuous_mobile/solana/codec.dart';
import 'package:continuous_mobile/solana/legacy_tx.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('system program pubkey is 32 zero bytes', () {
    final raw = base58Decode('11111111111111111111111111111111');
    expect(raw, List<int>.filled(32, 0));
    expect(base58Encode(raw), '11111111111111111111111111111111');
  });

  test('mask packs selected cells into little-endian bits', () {
    expect(hexMaskFromBits([true, false, false, false, false, false, false, false, true]), '0101');
  });

  test('legacy transaction reserves one blank signature for the fee payer', () {
    final payer = Uint8List(32)..fillRange(0, 32, 1);
    final program = Uint8List(32)..fillRange(0, 32, 2);
    final other = Uint8List(32)..fillRange(0, 32, 3);
    final hash = Uint8List(32)..fillRange(0, 32, 9);
    final raw = compileLegacy(
      feePayer: payer,
      recentBlockhash: hash,
      instructions: [
        CompiledIx(
          programId: program,
          keys: [AccountMeta(other, signer: false, writable: true)],
          data: Uint8List.fromList([1, 2, 3]),
        ),
      ],
    );
    expect(raw.first, 1);
    expect(raw.sublist(1, 65), List<int>.filled(64, 0));
    expect(raw.sublist(65, 68), [1, 0, 1]);
  });
}
