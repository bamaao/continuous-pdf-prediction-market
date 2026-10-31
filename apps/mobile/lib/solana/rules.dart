import 'dart:typed_data';

import 'codec.dart';
import 'pda.dart';

class NonceChoice {
  const NonceChoice({required this.nonce, required this.chainLast, required this.replay});

  final int nonce;
  final int chainLast;
  final bool replay;
}

/// Next buy uses chain `last + 1`. A stored nonce at or below `last` already filled.
NonceChoice planNonce({required int chainLast, required int stored}) {
  final next = chainLast + 1;
  if (stored <= chainLast && chainLast > 0) {
    return NonceChoice(nonce: next, chainLast: chainLast, replay: true);
  }
  return NonceChoice(nonce: next, chainLast: chainLast, replay: false);
}

bool nonceAdvanced({required int before, required int after}) => after > before;

int outcomeKind(int family) {
  switch (family) {
    case 0:
      return 0;
    case 3:
      return 2;
    case 4:
      return 4;
    default:
      return 1;
  }
}

bool outcomeUsesMilli(int family) => family == 1 || family == 2;

int outcomeValue({required int family, required String raw}) {
  if (family == 4) {
    final value = int.tryParse(raw.trim());
    if (value != 0 && value != 1) {
      throw StateError('Bernoulli 结果只能是 0 或 1');
    }
    return value!;
  }
  if (family == 1 || family == 2) {
    final value = double.tryParse(raw.trim());
    if (value == null) throw StateError('填写结果');
    return (value * 1000).round();
  }
  return int.tryParse(raw.trim()) ?? 0;
}

int outcomeValueB({required int family, required String rawB}) {
  if (family != 0) return 0;
  return int.tryParse(rawB.trim()) ?? 0;
}

/// Pad to `ceil(n / 8)` bytes. Cells past the visible list stay unset.
String hexMaskForSelection(List<bool> cells, int n) {
  final width = n > 0 ? n : cells.length;
  final bits = List<bool>.filled(width, false);
  for (var i = 0; i < cells.length && i < width; i++) {
    bits[i] = cells[i];
  }
  return hexMaskFromBits(bits);
}

bool alreadyExists(Object error) {
  final text = '$error'.toLowerCase();
  return text.contains('already in use') || text.contains('already exists') || text.contains('already initialized');
}

bool releaseAllows(String url, {required bool release}) {
  if (!release) return true;
  return Uri.tryParse(url)?.scheme == 'https';
}

void ensureTransport(String url, {required bool release}) {
  if (!releaseAllows(url, release: release)) {
    throw StateError('正式包只能访问 HTTPS');
  }
}

bool programAllowed(String programId) => allowedPrograms.contains(programId);

/// Shard 0 is sealed when `p0.len` covers the local cells and `z` is non-zero.
bool gridSealed(Uint8List data, int n) {
  const header = 61;
  const zOff = 45;
  if (n < 2 || data.length < header + 4 || data.length < zOff + 16) return false;
  final p0 = data[header] | (data[header + 1] << 8) | (data[header + 2] << 16) | (data[header + 3] << 24);
  var nonzero = false;
  for (var i = 0; i < 16; i++) {
    if (data[zOff + i] != 0) nonzero = true;
  }
  final local = n < 16 ? n : 16;
  return p0 == local && nonzero;
}
