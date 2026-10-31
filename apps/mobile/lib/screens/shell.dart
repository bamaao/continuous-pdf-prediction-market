import 'package:flutter/material.dart';

import '../ui/desk_theme.dart';
import '../wallet/session.dart';
import 'auctions_page.dart';
import 'lobby_page.dart';
import 'lp_page.dart';
import 'more_page.dart';
import 'portfolio_page.dart';

class DeskScope extends InheritedNotifier<WalletSession> {
  const DeskScope({required WalletSession session, required super.child, super.key}) : super(notifier: session);

  static WalletSession of(BuildContext context) {
    final scope = context.dependOnInheritedWidgetOfExactType<DeskScope>();
    assert(scope != null, 'DeskScope missing');
    return scope!.notifier!;
  }
}

class DeskShell extends StatefulWidget {
  const DeskShell({super.key});

  @override
  State<DeskShell> createState() => _DeskShellState();
}

class _DeskShellState extends State<DeskShell> {
  var _index = 0;

  @override
  Widget build(BuildContext context) {
    final wallet = DeskScope.of(context);
    const pages = [LobbyPage(), AuctionsPage(), PortfolioPage(), LpPage(), MorePage()];
    return Scaffold(
      appBar: AppBar(
        title: const Text('Continuous'),
        actions: [
          TextButton(
            onPressed: wallet.busy
                ? null
                : () async {
                    if (wallet.connected) {
                      await wallet.disconnect();
                      return;
                    }
                    try {
                      await wallet.connect();
                    } catch (_) {
                      // WalletSession.note already holds the failure.
                    }
                  },
            child: Text(wallet.connected ? shortKey(wallet.address) : '连接钱包'),
          ),
        ],
      ),
      body: Column(
        children: [
          if (wallet.note.isNotEmpty)
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 8, 16, 0),
              child: Align(
                alignment: Alignment.centerLeft,
                child: Text(wallet.note, style: const TextStyle(color: amber, fontSize: 13)),
              ),
            ),
          Expanded(child: pages[_index]),
        ],
      ),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _index,
        onDestinationSelected: (value) => setState(() => _index = value),
        destinations: const [
          NavigationDestination(icon: Icon(Icons.storefront_outlined), label: '大厅'),
          NavigationDestination(icon: Icon(Icons.gavel_outlined), label: '拍卖'),
          NavigationDestination(icon: Icon(Icons.account_balance_wallet_outlined), label: '组合'),
          NavigationDestination(icon: Icon(Icons.layers_outlined), label: '做市'),
          NavigationDestination(icon: Icon(Icons.more_horiz), label: '更多'),
        ],
      ),
    );
  }
}

class DeskMessage extends StatelessWidget {
  const DeskMessage(this.text, {super.key});

  final String text;

  @override
  Widget build(BuildContext context) {
    if (text.isEmpty) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: Text(text, style: const TextStyle(color: amber, fontSize: 13)),
    );
  }
}
