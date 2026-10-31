import 'package:shared_preferences/shared_preferences.dart';

class NonceBook {
  NonceBook({required Future<int?> Function(String key) read, required Future<void> Function(String key, int value) write})
    : _read = read,
      _write = write;

  final Future<int?> Function(String key) _read;
  final Future<void> Function(String key, int value) _write;

  static Future<NonceBook> prefs() async {
    final prefs = await SharedPreferences.getInstance();
    return NonceBook(
      read: (key) async => prefs.getInt(key),
      write: (key, value) async {
        await prefs.setInt(key, value);
      },
    );
  }

  String _key(String owner, String market) => 'cpm.nonce.$owner.$market';

  Future<int> stored(String owner, String market) async => await _read(_key(owner, market)) ?? 1;

  Future<void> save(String owner, String market, int next) => _write(_key(owner, market), next);
}
