import 'package:flutter/material.dart';

import '../solana/rules.dart';
import '../ui/desk_theme.dart';
import 'shell.dart';

class AuctionPage extends StatefulWidget {
  const AuctionPage({required this.market, required this.title, super.key});

  final String market;
  final String title;

  @override
  State<AuctionPage> createState() => _AuctionPageState();
}

class _AuctionPageState extends State<AuctionPage> {
  final _capacity = TextEditingController(text: '100');
  final _premium = TextEditingController(text: '10');
  final _share = TextEditingController(text: '0');
  final _layer = TextEditingController(text: '1');
  String _note = '';
  var _busy = false;
  var _known = false;
  var _closeTs = 0;
  var _lockTs = 0;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _loadClock();
  }

  var _clockStarted = false;

  Future<void> _loadClock() async {
    if (_clockStarted) return;
    _clockStarted = true;
    try {
      final info = await DeskScope.of(context).api.info(widget.market);
      if (!mounted) return;
      setState(() {
        _closeTs = asInt(info['close_ts']);
        _lockTs = asInt(info['risk_lock_ts']);
        _known = true;
      });
    } catch (e) {
      if (mounted) setState(() => _note = '$e');
    }
  }

  bool get _canCancel {
    if (!_known) return false;
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final lock = _lockTs > 0 ? _lockTs : _closeTs;
    if (lock <= 0) return true;
    return now < lock;
  }

  @override
  void dispose() {
    _capacity.dispose();
    _premium.dispose();
    _share.dispose();
    _layer.dispose();
    super.dispose();
  }

  Future<void> _quote() async {
    final wallet = DeskScope.of(context);
    if (!wallet.connected) {
      setState(() => _note = '先连接钱包');
      return;
    }
    setState(() {
      _busy = true;
      _note = '正在签名…';
    });
    try {
      try {
        await wallet.submit({'op': 'risk_open_book', 'owner': wallet.address, 'market': widget.market});
      } catch (e) {
        if (!alreadyExists(e)) rethrow;
      }
      final sig = await wallet.submit({
        'op': 'risk_quote',
        'owner': wallet.address,
        'market': widget.market,
        'layer': int.tryParse(_layer.text) ?? 1,
        'capacity': int.tryParse(_capacity.text) ?? 0,
        'premium': int.tryParse(_premium.text) ?? 0,
        'profit_share_bps': int.tryParse(_share.text) ?? 0,
      });
      setState(() => _note = '报价已确认 $sig');
    } catch (e) {
      setState(() => _note = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _cancel() async {
    final wallet = DeskScope.of(context);
    if (!wallet.connected) {
      setState(() => _note = '先连接钱包');
      return;
    }
    if (!_canCancel) {
      setState(() => _note = '已过风险锁定时间，不能撤销未成交报价');
      return;
    }
    setState(() => _busy = true);
    try {
      final sig = await wallet.submit({
        'op': 'cancel_unfilled',
        'owner': wallet.address,
        'market': widget.market,
        'layer': int.tryParse(_layer.text) ?? 1,
      });
      setState(() => _note = '已撤销 $sig');
    } catch (e) {
      setState(() => _note = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: Text(widget.title)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text(shortKey(widget.market)),
          const SizedBox(height: 12),
          TextField(controller: _layer, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: '层')),
          TextField(controller: _capacity, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: '容量 USDC')),
          TextField(controller: _premium, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: '保费 USDC')),
          TextField(controller: _share, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: '利润分成 bps')),
          const SizedBox(height: 12),
          DeskMessage(_note),
          Row(
            children: [
              FilledButton(onPressed: _busy ? null : _quote, child: const Text('报价')),
              const SizedBox(width: 8),
              if (_canCancel) OutlinedButton(onPressed: _busy ? null : _cancel, child: const Text('撤销未成交')),
            ],
          ),
        ],
      ),
    );
  }
}
