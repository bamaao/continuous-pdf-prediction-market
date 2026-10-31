import 'package:flutter/material.dart';

import '../ui/desk_theme.dart';
import 'shell.dart';

class PortfolioPage extends StatefulWidget {
  const PortfolioPage({super.key});

  @override
  State<PortfolioPage> createState() => _PortfolioPageState();
}

class _PortfolioPageState extends State<PortfolioPage> {
  Map<String, dynamic>? _vault;
  List<Map<String, dynamic>> _items = const [];
  final _amount = TextEditingController(text: '100');
  String _note = '';
  var _busy = false;

  @override
  void dispose() {
    _amount.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    final wallet = DeskScope.of(context);
    if (!wallet.connected) {
      setState(() {
        _vault = null;
        _items = const [];
        _note = '连接钱包后看得到保证金和持仓。';
      });
      return;
    }
    try {
      final owner = wallet.address!;
      final vault = await wallet.api.vault(owner);
      final items = await wallet.api.positions(owner);
      if (mounted) {
        setState(() {
          _vault = vault;
          _items = items;
          _note = '';
        });
      }
    } catch (e) {
      if (mounted) setState(() => _note = '$e');
    }
  }

  String? _loadedFor;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    final address = DeskScope.of(context).address ?? '';
    if (_loadedFor == address) return;
    _loadedFor = address;
    _load();
  }

  Future<void> _cash(String op) async {
    final wallet = DeskScope.of(context);
    final amount = int.tryParse(_amount.text.trim()) ?? 0;
    if (!wallet.connected || amount <= 0) {
      setState(() => _note = '填写大于 0 的 USDC，并连接钱包');
      return;
    }
    final available = asInt(_vault?['available']);
    final reserved = asInt(_vault?['reserved']);
    final pending = asInt(_vault?['trade_pending']);
    final free = asInt(_vault?['free'], available - reserved - pending);
    if (op == 'withdraw' && amount > free) {
      setState(() => _note = '可提现 $free USDC');
      return;
    }
    setState(() {
      _busy = true;
      _note = '正在签名…';
    });
    try {
      if (op == 'deposit') {
        try {
          await wallet.submit({'op': 'create_ata', 'owner': wallet.address});
        } catch (_) {}
      }
      final sig = await wallet.submit({'op': op, 'owner': wallet.address, 'amount': amount});
      setState(() => _note = '$op 已确认 $sig');
      await _load();
    } catch (e) {
      setState(() => _note = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _claim(Map<String, dynamic> ticket) async {
    final wallet = DeskScope.of(context);
    final refund = ticket['prompt'] == 'unclaimed_refund';
    final skellam = ticket['ticket_kind'] == 'skellam';
    setState(() => _busy = true);
    try {
      final sig = await wallet.submit({
        'op': refund ? 'refund' : (skellam ? 'payout_skellam' : 'payout'),
        'owner': wallet.address,
        'market': ticket['market'],
        'mask': ticket['mask'],
        'position': ticket['position'],
        'set_hash': ticket['set_hash'],
        'kind': ticket['skellam_kind'],
        'value': ticket['a'],
        'value_b': ticket['b'],
        'family': ticket['family'],
      });
      setState(() => _note = '领取已确认 $sig');
      await _load();
    } catch (e) {
      setState(() => _note = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final vault = _vault;
    final available = asInt(vault?['available']);
    final reserved = asInt(vault?['reserved']);
    final pending = asInt(vault?['trade_pending']);
    final free = asInt(vault?['free'], available - reserved - pending);
    return RefreshIndicator(
      onRefresh: _load,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          const Text('组合', style: TextStyle(fontSize: 28)),
          const SizedBox(height: 8),
          const Text('买入使用钱包签名。Session 预算不能在这里卖出，也不能把未用保证金转走。'),
          const SizedBox(height: 12),
          Text('钱包 ATA ${asInt(vault?['wallet_usdc'])} USDC'),
          Text('金库可用 $available · 占用 $reserved · Session 预算 $pending'),
          Text('可提现 $free USDC'),
          const SizedBox(height: 12),
          TextField(controller: _amount, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: 'USDC 数量')),
          const SizedBox(height: 8),
          Row(
            children: [
              FilledButton(onPressed: _busy ? null : () => _cash('deposit'), child: const Text('存入')),
              const SizedBox(width: 8),
              OutlinedButton(onPressed: _busy ? null : () => _cash('withdraw'), child: const Text('提现')),
            ],
          ),
          const SizedBox(height: 12),
          DeskMessage(_note),
          for (final ticket in _items)
            Card(
              color: const Color(0xFF1C1914),
              child: ListTile(
                title: Text(asText(ticket['title'], shortKey(asText(ticket['market'])))),
                subtitle: Text('${ticket['prompt']} · ${asInt(ticket['shares'])} 份'),
                trailing: ticket['prompt'] == 'unclaimed_settle' || ticket['prompt'] == 'unclaimed_refund'
                    ? TextButton(onPressed: _busy ? null : () => _claim(ticket), child: const Text('领取'))
                    : null,
              ),
            ),
        ],
      ),
    );
  }
}
