import 'package:flutter/material.dart';

import 'committee_page.dart';
import 'create_page.dart';

class MorePage extends StatelessWidget {
  const MorePage({super.key});

  @override
  Widget build(BuildContext context) {
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        const Text('更多', style: TextStyle(fontSize: 28)),
        const SizedBox(height: 12),
        ListTile(
          title: const Text('创建盘口'),
          subtitle: const Text('开一个 Bernoulli 市场并封板'),
          trailing: const Icon(Icons.chevron_right),
          onTap: () => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => const CreatePage())),
        ),
        ListTile(
          title: const Text('委员会'),
          subtitle: const Text('查看当前委员名册'),
          trailing: const Icon(Icons.chevron_right),
          onTap: () => Navigator.of(context).push(MaterialPageRoute<void>(builder: (_) => const CommitteePage())),
        ),
      ],
    );
  }
}
