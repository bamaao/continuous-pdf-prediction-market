import 'package:flutter/material.dart';

import '../ui/desk_theme.dart';
import 'shell.dart';

class LpPage extends StatefulWidget {
  const LpPage({super.key});

  @override
  State<LpPage> createState() => _LpPageState();
}

class _LpPageState extends State<LpPage> {
  List<Map<String, dynamic>> _items = const [];
  String _note = '';
  String? _loadedFor;
  var _busy = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final address = DeskScope.of(context).address ?? '';
    if (_loadedFor == address) return;
    _loadedFor = address;
    _load();
  }

  Future<void> _load() async {
    final wallet = DeskScope.of(context);
    if (!wallet.connected) {
      setState(() {
        _items = const [];
        _note = '连接钱包后看到自己的风险报价。';
      });
      return;
    }
    try {
      final book = await wallet.api.risk(wallet.address!);
      if (mounted) {
        setState(() {
          _items = (book['items'] as List? ?? const []).cast<Map<String, dynamic>>();
          _note = '';
        });
      }
    } catch (e) {
      if (mounted) setState(() => _note = '$e');
    }
  }

  bool _canCancel(Map<String, dynamic> row) {
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    final close = asInt(row['close_ts']);
    final lock = asInt(row['risk_lock_ts'], close);
    return close > 0 && lock > 0 && now < close && now < lock;
  }

  Future<void> _act(String op, Map<String, dynamic> row) async {
    final wallet = DeskScope.of(context);
    setState(() {
      _busy = true;
      _note = '正在签名…';
    });
    try {
      final sig = await wallet.submit({
        'op': op,
        'owner': wallet.address,
        'market': row['market'],
        'layer': row['layer'],
        'weight_sum': asInt(row['weight_sum'], 1),
      });
      setState(() => _note = '$op 已确认 $sig');
      _loadedFor = null;
      await _load();
    } catch (e) {
      setState(() => _note = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return RefreshIndicator(
      onRefresh: () async {
        _loadedFor = null;
        await _load();
      },
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          const Text('做市', style: TextStyle(fontSize: 28)),
          const SizedBox(height: 8),
          const Text('领取保费、释放报价、支付盈余都由这个钱包签名。锁仓后不能撤销未成交部分。'),
          const SizedBox(height: 12),
          DeskMessage(_note),
          if (_items.isEmpty) const Text('没有报价'),
          for (final row in _items)
            Card(
              color: const Color(0xFF1C1914),
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(asText(row['title'], shortKey(asText(row['market'])))),
                    Text('层 ${asInt(row['layer'])} · 容量 ${asInt(row['capacity'])}'),
                    Wrap(
                      spacing: 8,
                      children: [
                        if (_canCancel(row))
                          TextButton(onPressed: _busy ? null : () => _act('cancel_unfilled', row), child: const Text('撤销')),
                        TextButton(onPressed: _busy ? null : () => _act('draw_lp', row), child: const Text('领取保费')),
                        TextButton(onPressed: _busy ? null : () => _act('release_lp', row), child: const Text('释放')),
                        TextButton(onPressed: _busy ? null : () => _act('pay_surplus_lp', row), child: const Text('支付盈余')),
                      ],
                    ),
                  ],
                ),
              ),
            ),
        ],
      ),
    );
  }
}
