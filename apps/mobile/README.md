# Continuous Android

Flutter 原生交易台。大厅、盘口、买入、组合、拍卖、做市、创建和决议都在应用里。交易指令向市场接口索取，应用在本地拼成 Solana 交易，再交给本机 Mobile Wallet Adapter 签名，然后发到配置的 RPC。

```bash
cd apps/mobile
flutter run --dart-define=MARKET_API=http://10.0.2.2:8080 --dart-define=RPC_URL=http://10.0.2.2:8899
```

`10.0.2.2` 是模拟器看到的电脑。真机改成电脑的局域网地址。手机上需要已安装支持 MWA 的钱包。

`SOLANA_CLUSTER` 只用于钱包关联提示，默认 `devnet`。签名结果仍发到 `RPC_URL`。

上架 Solana dApp Store 用 release 包。正式包只访问 HTTPS，模拟器上的明文地址要在 debug 运行时用。签名读取 `android/key.properties`，密钥库不进 git。商店文案和图片在 `store/`。条款与隐私政策在网站的 `/terms` 和 `/privacy`，部署到公网 HTTPS 后填进发布者后台。

```bash
flutter build apk --release
```
