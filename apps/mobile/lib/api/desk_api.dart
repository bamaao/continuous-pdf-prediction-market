import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:http/http.dart' as http;

import '../solana/codec.dart';
import '../solana/legacy_tx.dart';
import '../solana/rules.dart';

class ComposeIx {
  ComposeIx({required this.programId, required this.keys, required this.data});

  final String programId;
  final List<ComposeKey> keys;
  final Uint8List data;

  CompiledIx toCompiled() {
    return CompiledIx(
      programId: base58Decode(programId),
      keys: keys
          .map((k) => AccountMeta(base58Decode(k.pubkey), signer: k.signer, writable: k.writable))
          .toList(),
      data: data,
    );
  }
}

class ComposeKey {
  ComposeKey({required this.pubkey, required this.signer, required this.writable});

  final String pubkey;
  final bool signer;
  final bool writable;
}

class DeskApi {
  DeskApi(this.base, {bool? release}) : _release = release ?? kReleaseMode;

  final String base;
  final bool _release;

  Future<Map<String, dynamic>> markets({String q = '', int page = 1, String status = '1'}) {
    return _json('/v1/markets?status=$status&page=$page&limit=20${q.isEmpty ? '' : '&q=${Uri.encodeQueryComponent(q)}'}');
  }

  Future<Map<String, dynamic>> auctions({int page = 1}) {
    return _json('/v1/auctions?page=$page&limit=20');
  }

  Future<Map<String, dynamic>> info(String market) => _json('/v1/markets/$market/info');

  Future<Map<String, dynamic>> quote(String market, String mask, int shares) {
    return _json('/v1/markets/$market/quote?mask=$mask&shares=$shares');
  }

  Future<Map<String, dynamic>?> vault(String owner) async {
    ensureTransport(base, release: _release);
    final response = await http.get(Uri.parse('$base/v1/owners/$owner/vault'));
    if (response.statusCode == 404) return null;
    return _decode(response);
  }

  Future<List<Map<String, dynamic>>> positions(String owner) async {
    final out = <Map<String, dynamic>>[];
    var page = 1;
    while (page <= 50) {
      final book = await _json('/v1/owners/$owner/positions?limit=100&page=$page');
      final items = (book['items'] as List? ?? const []).whereType<Map>().map((item) => Map<String, dynamic>.from(item)).toList();
      out.addAll(items);
      final pages = book['pages'] is num ? (book['pages'] as num).toInt() : 1;
      if (items.isEmpty || page >= pages) break;
      page++;
    }
    return out;
  }

  Future<Map<String, dynamic>> risk(String owner) => _json('/v1/owners/$owner/risk');

  Future<Map<String, dynamic>?> committee() async {
    ensureTransport(base, release: _release);
    final response = await http.get(Uri.parse('$base/v1/committee'));
    if (response.statusCode == 404) return null;
    return _decode(response);
  }

  Future<Map<String, dynamic>?> resolution(String market) async {
    ensureTransport(base, release: _release);
    final response = await http.get(Uri.parse('$base/v1/markets/$market/resolution'));
    if (response.statusCode == 404) return null;
    return _decode(response);
  }

  Future<List<ComposeIx>> compose(Map<String, Object?> body) async {
    ensureTransport(base, release: _release);
    final response = await http.post(
      Uri.parse('$base/v1/compose'),
      headers: {'content-type': 'application/json'},
      body: jsonEncode(body),
    );
    final decoded = _decode(response);
    final many = decoded['ixs'];
    if (many is List && many.isNotEmpty) {
      return many.map((item) => _ix(item as Map<String, dynamic>)).toList();
    }
    return [_ix(decoded)];
  }

  Future<void> saveListing(Map<String, Object?> body) async {
    ensureTransport(base, release: _release);
    final response = await http.post(
      Uri.parse('$base/v1/listings'),
      headers: {'content-type': 'application/json'},
      body: jsonEncode(body),
    );
    if (response.statusCode == 409) return;
    _decode(response);
  }

  ComposeIx _ix(Map<String, dynamic> raw) {
    final keys = (raw['keys'] as List? ?? const []).map((item) {
      final key = item as Map<String, dynamic>;
      return ComposeKey(
        pubkey: key['pubkey'] as String,
        signer: key['is_signer'] == true,
        writable: key['is_writable'] == true,
      );
    }).toList();
    return ComposeIx(
      programId: raw['program_id'] as String,
      keys: keys,
      data: base64Decode(raw['data_b64'] as String),
    );
  }

  Future<Map<String, dynamic>> _json(String path) async {
    ensureTransport(base, release: _release);
    return _decode(await http.get(Uri.parse('$base$path')));
  }

  Map<String, dynamic> _decode(http.Response response) {
    if (response.statusCode < 200 || response.statusCode >= 300) {
      throw StateError('接口 ${response.statusCode} ${response.body}');
    }
    final body = jsonDecode(response.body);
    if (body is Map<String, dynamic>) return body;
    if (body is List) return {'items': body};
    throw StateError('接口返回了无法识别的内容');
  }
}
