import 'dart:typed_data';

import 'package:continuous_mobile/solana/codec.dart';
import 'package:continuous_mobile/solana/pda.dart';
import 'package:continuous_mobile/solana/rules.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('nonce pda and grid pda match solders', () {
    final owner = Uint8List(32)..fillRange(0, 32, 1);
    final market = Uint8List(32)..fillRange(0, 32, 2);
    expect(base58Encode(owner), '4vJ9JU1bJJE96FWSJKvHsmmFADCg4gpZQff4P3bkLKi');
    expect(base58Encode(noncePda(owner, market)), 'B9LQh4CU9vT8TjR5tXJ8qpJwDuQaL3w9SgwFtNqhg5Dn');
    expect(base58Encode(gridPda(market)), 'HWwxSnwyA7kyNWWTotBHiVELYpMgPtgX2aE8VEsyvkNv');
  });

  test('a stored nonce at or below the chain last is a replay', () {
    final again = planNonce(chainLast: 5, stored: 1);
    expect(again.replay, isTrue);
    expect(again.nonce, 6);
    final next = planNonce(chainLast: 5, stored: 6);
    expect(next.replay, isFalse);
    expect(next.nonce, 6);
    final first = planNonce(chainLast: 0, stored: 1);
    expect(first.replay, isFalse);
    expect(first.nonce, 1);
    expect(nonceAdvanced(before: 5, after: 5), isFalse);
    expect(nonceAdvanced(before: 5, after: 6), isTrue);
  });

  test('outcome kind follows the family', () {
    expect(outcomeKind(0), 0);
    expect(outcomeKind(1), 1);
    expect(outcomeKind(2), 1);
    expect(outcomeKind(3), 2);
    expect(outcomeKind(4), 4);
    expect(outcomeValue(family: 4, raw: '1'), 1);
    expect(() => outcomeValue(family: 4, raw: '2'), throwsStateError);
    expect(outcomeValue(family: 1, raw: '2.5'), 2500);
    expect(outcomeUsesMilli(1), isTrue);
    expect(outcomeUsesMilli(4), isFalse);
  });

  test('mask is padded to the chain length', () {
    final cells = List<bool>.filled(64, false)..[0] = true;
    expect(hexMaskForSelection(cells, 80), '01${'00' * 9}');
    expect(hexMaskForSelection(const [true, false], 2), '01');
  });

  test('only an existing account is ignored, and release rejects cleartext', () {
    expect(alreadyExists(StateError('Account already in use')), isTrue);
    expect(alreadyExists(StateError('custom program error: 0x0')), isFalse);
    expect(releaseAllows('http://10.0.2.2:8080', release: false), isTrue);
    expect(releaseAllows('http://10.0.2.2:8080', release: true), isFalse);
    expect(releaseAllows('https://continuous.market', release: true), isTrue);
    expect(programAllowed(marketProgram), isTrue);
    expect(programAllowed('11111111111111111111111111111112'), isFalse);
  });

  test('a sealed shard has local cells and a non-zero z', () {
    final data = Uint8List(80);
    data[45] = 1;
    data[61] = 2;
    expect(gridSealed(data, 2), isTrue);
    data[61] = 1;
    expect(gridSealed(data, 2), isFalse);
  });
}
