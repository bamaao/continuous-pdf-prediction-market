import 'package:flutter/material.dart';

import '../ui/desk_theme.dart';
import 'shell.dart';

class CommitteePage extends StatefulWidget {
  const CommitteePage({super.key});

  @override
  State<CommitteePage> createState() => _CommitteePageState();
}

class _CommitteePageState extends State<CommitteePage> {
  Map<String, dynamic>? _committee;
  String _note = '';

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _load();
  }

  var _started = false;

  Future<void> _load() async {
    if (_started) return;
    _started = true;
    try {
      final committee = await DeskScope.of(context).api.committee();
      if (mounted) setState(() => _committee = committee);
    } catch (e) {
      if (mounted) setState(() => _note = '$e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final members = (_committee?['members'] as List? ?? const []).map((item) => item.toString()).toList();
    return Scaffold(
      appBar: AppBar(title: const Text('委员会')),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text('成员 ${members.length} · m ${asInt(_committee?['m'])} / n ${asInt(_committee?['n'])}'),
          const SizedBox(height: 8),
          for (final member in members) Text(shortKey(member), style: const TextStyle(fontFamily: 'monospace')),
          if (members.isEmpty) const Text('还没有委员会名册'),
          const SizedBox(height: 12),
          DeskMessage(_note),
          const Text('具体市场的开窗、提案和结算在盘口页的「决议与结算」。'),
        ],
      ),
    );
  }
}
