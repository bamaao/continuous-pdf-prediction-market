/// Local services as seen from an Android emulator. Override with --dart-define.
class AppConfig {
  static const api = String.fromEnvironment('MARKET_API', defaultValue: 'http://10.0.2.2:8080');
  static const rpc = String.fromEnvironment('RPC_URL', defaultValue: 'http://10.0.2.2:8899');
  static const cluster = String.fromEnvironment('SOLANA_CLUSTER', defaultValue: 'devnet');
  static const identity = 'https://continuous.market';
}
