import 'package:flutter/material.dart';

import '../solana/codec.dart';
import '../ui/desk_theme.dart';
import 'market_page.dart';
import 'shell.dart';

class LobbyPage extends StatefulWidget {
  const LobbyPage({super.key});

  @override
  State<LobbyPage> createState() => _LobbyPageState();
}

class _LobbyPageState extends State<LobbyPage> {
  final _query = TextEditingController();
  List<Map<String, dynamic>> _items = const [];
  String _error = '';
  String _status = '1';
  var _loading = true;
  var _started = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_started) return;
    _started = true;
    _load();
  }

  @override
  void dispose() {
    _query.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() {
      _loading = true;
      _error = '';
    });
    try {
      final page = await DeskScope.of(context).api.markets(q: _query.text.trim(), status: _status);
      final items = (page['items'] as List? ?? const []).cast<Map<String, dynamic>>();
      if (mounted) {
        setState(() {
          _items = items;
          _loading = false;
        });
      }
    } catch (e) {
      if (mounted) {
        setState(() {
          _error = '$e';
          _loading = false;
        });
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return RefreshIndicator(
      onRefresh: _load,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          const Text('交易中的市场', style: TextStyle(fontSize: 28, color: paper)),
          const SizedBox(height: 8),
          const Text('价格是概率。覆盖率是风险提示，不是折扣。命中赔付按面值，再乘结算 ρ。', style: TextStyle(color: paper, height: 1.4)),
          const SizedBox(height: 12),
          Row(
            children: [
              Expanded(
                child: TextField(
                  controller: _query,
                  decoration: const InputDecoration(labelText: '搜索名称或标签'),
                  onSubmitted: (_) => _load(),
                ),
              ),
              const SizedBox(width: 8),
              IconButton(onPressed: _load, icon: const Icon(Icons.search, color: amber)),
            ],
          ),
          const SizedBox(height: 12),
          Wrap(
            spacing: 6,
            children: [
              for (final entry in const {'1': '交易中', '2': '停牌', '3': '已结算', '4': '作废', 'all': '全部'}.entries)
                ChoiceChip(
                  label: Text(entry.value),
                  selected: _status == entry.key,
                  onSelected: (_) {
                    setState(() => _status = entry.key);
                    _load();
                  },
                ),
            ],
          ),
          const SizedBox(height: 12),
          DeskMessage(_error),
          if (_loading) const LinearProgressIndicator(color: amber),
          if (!_loading && _items.isEmpty) const Text('没有可交易的市场'),
          for (final item in _items) _MarketTile(item: item),
        ],
      ),
    );
  }
}

class _MarketTile extends StatelessWidget {
  const _MarketTile({required this.item});

  final Map<String, dynamic> item;

  @override
  Widget build(BuildContext context) {
    final market = asText(item['market']);
    final title = asText(item['title'], shortKey(market));
    final family = familyName(asInt(item['family']));
    final stake = asInt(item['stake_usdc']);
    return Card(
      color: const Color(0xFF1C1914),
      child: ListTile(
        title: Text(title, style: const TextStyle(color: paper)),
        subtitle: Text('$family · 押注 $stake USDC', style: const TextStyle(color: paper)),
        trailing: const Icon(Icons.chevron_right, color: amber),
        onTap: () {
          Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => MarketPage(market: market, title: title)));
        },
      ),
    );
  }
}
