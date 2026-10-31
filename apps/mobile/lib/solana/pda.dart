import 'dart:convert';
import 'dart:typed_data';

import 'package:crypto/crypto.dart';

import 'codec.dart';

const marketProgram = 'Market1111111111111111111111111111111111111';
const vaultProgram = 'VaULt11111111111111111111111111111111111111';
const resolutionProgram = 'Rso1111111111111111111111111111111111111111';
const riskProgram = 'Rsk1111111111111111111111111111111111111111';
const tokenProgram = 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA';
const associatedTokenProgram = 'ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL';
const systemProgram = '11111111111111111111111111111111';

const allowedPrograms = {
  marketProgram,
  vaultProgram,
  resolutionProgram,
  riskProgram,
  tokenProgram,
  associatedTokenProgram,
  systemProgram,
};

final _p = (BigInt.one << 255) - BigInt.from(19);
final _d = BigInt.parse('37095705934669439343138083508754565189542113879843219016388785533085940283555');
final _i = BigInt.parse('19681161376707505956807079304988542015446066515923890162744021073123829784752');

BigInt _mod(BigInt value) {
  final r = value % _p;
  return r.isNegative ? r + _p : r;
}

/// Edwards decompression. Matches Solana `Pubkey::is_on_curve`.
bool isOnCurve(Uint8List bytes) {
  if (bytes.length != 32) return false;
  var y = BigInt.zero;
  for (var i = 0; i < 32; i++) {
    y |= BigInt.from(bytes[i]) << (8 * i);
  }
  final sign = ((y >> 255) & BigInt.one).toInt();
  y &= (BigInt.one << 255) - BigInt.one;
  if (y >= _p) return false;
  final y2 = (y * y) % _p;
  final u = _mod(y2 - BigInt.one);
  final v = _mod(_d * y2 + BigInt.one);
  final x2 = (u * v.modPow(_p - BigInt.two, _p)) % _p;
  var x = x2.modPow((_p + BigInt.from(3)) ~/ BigInt.from(8), _p);
  if (_mod(x * x - x2) != BigInt.zero) {
    if (_mod(x * x + x2) != BigInt.zero) return false;
    x = (x * _i) % _p;
    if (_mod(x * x - x2) != BigInt.zero) return false;
  }
  if ((x & BigInt.one).toInt() != sign) x = _p - x;
  return true;
}

Uint8List? createProgramAddress(List<Uint8List> seeds, Uint8List programId) {
  final buf = BytesBuilder(copy: false);
  for (final seed in seeds) {
    if (seed.length > 32) return null;
    buf.add(seed);
  }
  buf.add(programId);
  buf.add(utf8.encode('ProgramDerivedAddress'));
  final hash = Uint8List.fromList(sha256.convert(buf.toBytes()).bytes);
  if (isOnCurve(hash)) return null;
  return hash;
}

Uint8List findProgramAddress(List<Uint8List> seeds, Uint8List programId) {
  for (var bump = 255; bump >= 0; bump--) {
    final found = createProgramAddress([...seeds, Uint8List.fromList([bump])], programId);
    if (found != null) return found;
  }
  throw StateError('找不到程序派生地址');
}

Uint8List noncePda(Uint8List owner, Uint8List market) {
  return findProgramAddress([
    Uint8List.fromList(utf8.encode('nonce')),
    owner,
    market,
  ], base58Decode(marketProgram));
}

Uint8List gridPda(Uint8List market) {
  return findProgramAddress([
    Uint8List.fromList(utf8.encode('grid')),
    market,
  ], base58Decode(marketProgram));
}
