import 'package:flutter/material.dart';

import '../solana/rules.dart';
import 'shell.dart';

class CreatePage extends StatefulWidget {
  const CreatePage({super.key});

  @override
  State<CreatePage> createState() => _CreatePageState();
}

class _CreatePageState extends State<CreatePage> {
  final _title = TextEditingController();
  final _topic = TextEditingController();
  final _tag = TextEditingController(text: 'binary');
  final _event = TextEditingController();
  String _note = '';
  var _busy = false;

  @override
  void dispose() {
    _title.dispose();
    _topic.dispose();
    _tag.dispose();
    _event.dispose();
    super.dispose();
  }

  Future<void> _existing(Future<String> Function() action) async {
    try {
      await action();
    } catch (e) {
      if (!alreadyExists(e)) rethrow;
    }
  }

  Future<void> _open() async {
    final wallet = DeskScope.of(context);
    if (!wallet.connected) {
      setState(() => _note = '先连接钱包');
      return;
    }
    if (_topic.text.trim().isEmpty || _tag.text.trim().isEmpty || _title.text.trim().isEmpty) {
      setState(() => _note = '填写标题、主题和标签');
      return;
    }
    final close = DateTime.now().millisecondsSinceEpoch ~/ 1000 + 86400;
    setState(() {
      _busy = true;
      _note = '正在创建 Bernoulli 盘口…';
    });
    try {
      final created = await wallet.api.compose({
        'op': 'create_bernoulli',
        'owner': wallet.address,
        'family': 4,
        'topic': _topic.text.trim(),
        'tag': _tag.text.trim(),
        'n': 2,
        'close_ts': close,
        'risk_lock_ts': close,
        'report_open_ts': close,
        'early_resolve': false,
      });
      final market = created.first.keys.length > 1 ? created.first.keys[1].pubkey : '';
      if (market.isEmpty) throw StateError('创建指令里没有市场地址');
      await _existing(() => wallet.submitIxs(created));
      await _existing(() => wallet.submit({'op': 'write_grid_mass', 'owner': wallet.address, 'market': market}));
      await _existing(() => wallet.submit({'op': 'seal_grid', 'owner': wallet.address, 'market': market, 'n': 2}));
      final sealed = await wallet.rpc.gridIsSealed(market, 2);
      if (!sealed) throw StateError('网格没有封板，没有写入上架');
      await _existing(() => wallet.submit({'op': 'fund_cm', 'owner': wallet.address, 'market': market, 'amount': 0}));
      await _existing(() => wallet.submit({'op': 'risk_open_book', 'owner': wallet.address, 'market': market}));
      await wallet.api.saveListing({
        'market': market,
        'title': _title.text.trim(),
        'tags': [_tag.text.trim()],
        'category': _tag.text.trim(),
        'topic': _topic.text.trim(),
        'tag': _tag.text.trim(),
        'event': _event.text.trim(),
        'description': _event.text.trim(),
        'source_locale': 'zh',
      });
      setState(() => _note = '盘口已开 $market');
    } catch (e) {
      setState(() => _note = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(title: const Text('创建')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          const Text('这里开一个两结果的 Bernoulli 盘口。创建、写入网格、封板、开金库和风险簿会连续签名。'),
          const SizedBox(height: 12),
          TextField(controller: _title, decoration: const InputDecoration(labelText: '标题')),
          TextField(controller: _topic, decoration: const InputDecoration(labelText: '主题')),
          TextField(controller: _tag, decoration: const InputDecoration(labelText: '标签')),
          TextField(controller: _event, decoration: const InputDecoration(labelText: '事件')),
          const SizedBox(height: 12),
          DeskMessage(_note),
          FilledButton(onPressed: _busy ? null : _open, child: const Text('开盘')),
        ],
      ),
    );
  }
}
