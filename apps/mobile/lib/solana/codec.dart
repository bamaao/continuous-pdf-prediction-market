import 'dart:typed_data';

const _alphabet = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

Uint8List base58Decode(String input) {
  var zeros = 0;
  while (zeros < input.length && input[zeros] == '1') {
    zeros++;
  }
  final size = ((input.length - zeros) * 138 ~/ 100) + 1;
  final b256 = List<int>.filled(size, 0);
  for (var i = zeros; i < input.length; i++) {
    var carry = _alphabet.indexOf(input[i]);
    if (carry < 0) {
      throw FormatException('bad base58');
    }
    for (var j = size - 1; j >= 0; j--) {
      carry += 58 * b256[j];
      b256[j] = carry & 0xff;
      carry >>= 8;
    }
  }
  var start = 0;
  while (start < b256.length && b256[start] == 0) {
    start++;
  }
  return Uint8List.fromList(<int>[...List<int>.filled(zeros, 0), ...b256.sublist(start)]);
}

String base58Encode(Uint8List bytes) {
  var zeros = 0;
  while (zeros < bytes.length && bytes[zeros] == 0) {
    zeros++;
  }
  final size = bytes.length * 138 ~/ 100 + 1;
  final b58 = List<int>.filled(size, 0);
  for (var i = zeros; i < bytes.length; i++) {
    var carry = bytes[i];
    for (var j = size - 1; j >= 0; j--) {
      carry += 256 * b58[j];
      b58[j] = carry % 58;
      carry ~/= 58;
    }
  }
  var start = 0;
  while (start < b58.length && b58[start] == 0) {
    start++;
  }
  return '${'1' * zeros}${b58.sublist(start).map((d) => _alphabet[d]).join()}';
}

void writeShortvec(BytesBuilder out, int length) {
  var n = length;
  while (true) {
    final elem = n & 0x7f;
    n >>= 7;
    if (n == 0) {
      out.addByte(elem);
      break;
    }
    out.addByte(elem | 0x80);
  }
}

String hexMaskFromBits(List<bool> bits) {
  final need = (bits.length + 7) >> 3;
  final bytes = Uint8List(need);
  for (var i = 0; i < bits.length; i++) {
    if (bits[i]) bytes[i >> 3] |= 1 << (i & 7);
  }
  return bytes.map((b) => b.toRadixString(16).padLeft(2, '0')).join();
}

String familyName(int family) {
  const names = ['Skellam', 'Gaussian', 'Lognormal', 'Dirichlet', 'Bernoulli'];
  if (family < 0 || family >= names.length) return 'family $family';
  return names[family];
}

String statusName(int status) {
  const names = ['', '交易中', '停牌', '已结算', '作废'];
  if (status < 0 || status >= names.length || names[status].isEmpty) return '状态 $status';
  return names[status];
}
