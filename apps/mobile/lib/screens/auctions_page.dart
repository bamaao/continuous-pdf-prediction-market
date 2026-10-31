import 'package:flutter/material.dart';

import '../ui/desk_theme.dart';
import 'auction_page.dart';
import 'shell.dart';

class AuctionsPage extends StatefulWidget {
  const AuctionsPage({super.key});

  @override
  State<AuctionsPage> createState() => _AuctionsPageState();
}

class _AuctionsPageState extends State<AuctionsPage> {
  List<Map<String, dynamic>> _items = const [];
  String _error = '';

  var _started = false;

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_started) return;
    _started = true;
    _load();
  }

  Future<void> _load() async {
    try {
      final page = await DeskScope.of(context).api.auctions();
      if (mounted) {
        setState(() {
          _items = (page['items'] as List? ?? const []).cast<Map<String, dynamic>>();
          _error = '';
        });
      }
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    return RefreshIndicator(
      onRefresh: _load,
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          const Text('风险拍卖', style: TextStyle(fontSize: 28)),
          const SizedBox(height: 8),
          const Text('报价补的是覆盖。交易截止和风险锁定之后，未成交报价不能再撤销。'),
          const SizedBox(height: 12),
          DeskMessage(_error),
          if (_items.isEmpty) const Text('没有进行中的拍卖'),
          for (final item in _items)
            Card(
              color: const Color(0xFF1C1914),
              child: ListTile(
                title: Text(asText(item['title'], shortKey(asText(item['market'])))),
                subtitle: Text('覆盖 ${asInt(item['coverage_bps'])} bps'),
                trailing: const Icon(Icons.chevron_right, color: amber),
                onTap: () {
                  final market = asText(item['market']);
                  Navigator.of(context).push(
                    MaterialPageRoute<void>(
                      builder: (_) => AuctionPage(market: market, title: asText(item['title'], shortKey(market))),
                    ),
                  );
                },
              ),
            ),
        ],
      ),
    );
  }
}
