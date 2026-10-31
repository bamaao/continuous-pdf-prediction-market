import 'dart:io';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:image/image.dart';

void main() {
  test('writes an opaque 512 icon, 1200x600 banner, and launcher icons', () {
    final icon = _mark(512, 512, banner: false);
    final banner = _mark(1200, 600, banner: true);
    _write('store/icon.png', icon);
    _write('store/banner.png', banner);
    for (final density in const {'mdpi': 48, 'hdpi': 72, 'xhdpi': 96, 'xxhdpi': 144, 'xxxhdpi': 192}.entries) {
      _write('android/app/src/main/res/mipmap-${density.key}/ic_launcher.png', copyResize(icon, width: density.value, height: density.value));
    }
    expect(icon.width, 512);
    expect(icon.height, 512);
    expect(banner.width, 1200);
    expect(banner.height, 600);
    expect(icon.numChannels, 3);
    expect(banner.numChannels, 3);
  });
}

Image _mark(int width, int height, {required bool banner}) {
  final image = Image(width: width, height: height, numChannels: 3);
  fill(image, color: ColorRgb8(0x14, 0x12, 0x0E));
  final left = banner ? (width * 0.08).round() : (width * 0.16).round();
  final right = banner ? (width * 0.46).round() : (width * 0.84).round();
  final base = (height * (banner ? 0.62 : 0.68)).round();
  final amp = (height * (banner ? 0.28 : 0.32)).round();
  var previousX = left;
  var previousY = base;
  for (var step = 0; step <= 80; step++) {
    final t = step / 80;
    final x = left + ((right - left) * t).round();
    final y = base - (amp * math.exp(-math.pow(t - 0.5, 2) / 0.045)).round();
    drawLine(image, x1: previousX, y1: previousY, x2: x, y2: y, color: ColorRgb8(0xE6, 0xA1, 0x5C), thickness: banner ? 8 : 18);
    previousX = x;
    previousY = y;
  }
  if (banner) {
    drawString(image, 'Continuous', font: arial48, x: (width * 0.52).round(), y: (height * 0.34).round(), color: ColorRgb8(0xF4, 0xEF, 0xE6));
    drawString(
      image,
      'PDF markets on Solana',
      font: arial24,
      x: (width * 0.52).round(),
      y: (height * 0.52).round(),
      color: ColorRgb8(0xE6, 0xA1, 0x5C),
    );
  }
  return image;
}

void _write(String path, Image image) {
  final bytes = Uint8List.fromList(encodePng(image));
  final file = File(path);
  file.parent.createSync(recursive: true);
  file.writeAsBytesSync(bytes, flush: true);
  final data = ByteData.sublistView(bytes);
  expect(data.getUint32(16), image.width);
  expect(data.getUint32(20), image.height);
  expect(bytes[25], 2);
}
