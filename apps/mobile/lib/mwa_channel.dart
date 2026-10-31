import 'package:flutter/services.dart';

class MwaChannel {
  static const _channel = MethodChannel('continuous/mwa');

  Future<String> authorize({required String cluster, required String origin}) async {
    final raw = await _channel.invokeMethod<Map<Object?, Object?>>('authorize', {
      'cluster': cluster,
      'origin': origin,
    });
    final key = raw?['publicKey'];
    if (key is! String || key.isEmpty) {
      throw StateError('wallet did not return a public key');
    }
    return key;
  }

  Future<List<String>> signTransactions(List<String> transactions) async {
    final raw = await _channel.invokeMethod<Map<Object?, Object?>>('signTransactions', {
      'transactions': transactions,
    });
    final signed = raw?['transactions'];
    if (signed is! List || signed.isEmpty) {
      throw StateError('wallet did not return signed transactions');
    }
    return signed.map((item) => item.toString()).toList();
  }

  Future<String> signMessage(String message) async {
    final raw = await _channel.invokeMethod<Map<Object?, Object?>>('signMessage', {
      'message': message,
    });
    final signature = raw?['signature'];
    if (signature is! String || signature.isEmpty) {
      throw StateError('wallet did not return a signature');
    }
    return signature;
  }

  Future<void> disconnect() async {
    await _channel.invokeMethod<void>('disconnect');
  }
}
