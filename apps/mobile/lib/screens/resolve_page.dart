import 'dart:convert';

import 'package:crypto/crypto.dart';
import 'package:flutter/material.dart';

import '../solana/codec.dart';
import '../solana/rules.dart';
import '../ui/desk_theme.dart';
import 'shell.dart';

class ResolvePage extends StatefulWidget {
  const ResolvePage({required this.market, required this.title, super.key});

  final String market;
  final String title;

  @override
  State<ResolvePage> createState() => _ResolvePageState();
}

class _ResolvePageState extends State<ResolvePage> {
  Map<String, dynamic>? _info;
  Map<String, dynamic>? _record;
  List<String> _roster = const [];
  final _value = TextEditingController(text: '1');
  final _away = TextEditingController(text: '0');
  final _evidence = TextEditingController();
  String _note = '';
  var _busy = false;
  var _started = false;

  @override
  void dispose() {
    _value.dispose();
    _away.dispose();
    _evidence.dispose();
    super.dispose();
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _load();
  }

  Future<void> _load() async {
    if (_started && _record != null) return;
    _started = true;
    try {
      final api = DeskScope.of(context).api;
      final info = await api.info(widget.market);
      final record = await api.resolution(widget.market);
      final committee = await api.committee();
      if (!mounted) return;
      setState(() {
        _info = info;
        _record = record;
        _roster = _strings(committee?['members']);
      });
    } catch (e) {
      if (mounted) setState(() => _note = '$e');
    }
  }

  int get _family => asInt(_record?['family'], asInt(_info?['family'], 4));

  int get _now => DateTime.now().millisecondsSinceEpoch ~/ 1000;

  List<String> _strings(Object? raw) {
    if (raw is! List) return const [];
    return raw.map((item) => item.toString()).toList();
  }

  String? _evidenceHex() {
    final text = _evidence.text.trim();
    if (text.isEmpty) return null;
    final hex = text.startsWith('0x') || text.startsWith('0X') ? text.substring(2) : text;
    if (RegExp(r'^[0-9a-fA-F]{64}$').hasMatch(hex)) return hex.toLowerCase();
    return sha256.convert(utf8.encode(text)).toString();
  }

  Future<String> _submit(String op, [Map<String, Object?> extra = const {}]) {
    final wallet = DeskScope.of(context);
    final family = _family;
    final proposes = op == 'submit_result' || op == 'challenge' || op == 'admin_submit_result';
    final value = proposes ? outcomeValue(family: family, raw: _value.text) : int.tryParse(_value.text.trim()) ?? 0;
    final valueB = proposes ? outcomeValueB(family: family, rawB: _away.text) : 0;
    return wallet.submit({
      'op': op,
      'owner': wallet.address,
      'market': widget.market,
      'family': family,
      'kind': outcomeKind(family),
      'milli': outcomeUsesMilli(family),
      'value': value,
      'value_b': valueB,
      if (_evidenceHex() != null) 'evidence_hex': _evidenceHex(),
      ...extra,
    });
  }

  Future<void> _try(String op, [Map<String, Object?> extra = const {}]) async {
    try {
      await _submit(op, extra);
    } catch (_) {}
  }

  Future<void> _run(String op, [Map<String, Object?> extra = const {}]) async {
    final wallet = DeskScope.of(context);
    if (!wallet.connected) {
      setState(() => _note = '先连接钱包');
      return;
    }
    final phase = asInt(_record?['phase']);
    final deadline = asInt(_record?['report_deadline']);
    final window = asInt(_record?['report_window_secs']);
    final missed = op == 'finalize' && phase == 0 && _now >= deadline + window;
    setState(() {
      _busy = true;
      _note = '正在签名…';
    });
    try {
      final sig = await _submit(op, extra);
      if ((op == 'finalize' || op == 'void_resolution' || op == 'admin_void_resolution') && !missed) {
        await _try('release_committee_bond');
      }
      if (op == 'void_resolution' || op == 'admin_void_resolution' || missed) {
        await _try('begin_refund');
      }
      if (op == 'admin_submit_result') {
        final holder = asText(_record?['bond_holder']);
        if (holder.isNotEmpty && holder != '11111111111111111111111111111111') {
          await _try('slash_committee_bond', {'trader': holder});
        }
      }
      setState(() => _note = '$op 已确认 $sig');
      _started = false;
      _record = null;
      await _load();
    } catch (e) {
      setState(() => _note = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final wallet = DeskScope.of(context).address ?? '';
    final record = _record;
    final phase = asInt(record?['phase']);
    final status = asInt(_info?['status']);
    final members = record == null ? _roster : _strings(record['members']);
    final member = wallet.isNotEmpty && members.contains(wallet);
    final creator = asText(_info?['creator']);
    final platform = asText(_info?['platform']);
    final isAdmin = wallet.isNotEmpty && platform.isNotEmpty && wallet == platform;
    final terminal = record != null && phase >= 3;
    final close = asInt(record?['close_ts'], asInt(_info?['close_ts']));
    final reportOpen = asInt(record?['report_open_ts'], asInt(_info?['report_open_ts'], close));
    final deadline = asInt(record?['report_deadline']);
    final window = asInt(record?['report_window_secs']);
    final graceEnd = deadline + window;
    final bondLocked = asInt(record?['bond_locked']) > 0 && record?['bond_slashed'] != true;
    final canOpen = record == null && wallet.isNotEmpty && (members.contains(wallet) || (creator.isNotEmpty && wallet == creator));
    final canPropose = member || (wallet.isNotEmpty && asText(record?['authorized_reporter']) == wallet && asText(record?['authorized_reporter']) != '11111111111111111111111111111111');
    final canLock = record != null && phase == 0 && !terminal && canPropose && !bondLocked && _now >= reportOpen && _now < deadline;
    final canSubmit = record != null && phase == 0 && canPropose && bondLocked && _now >= reportOpen && _now < deadline;
    final canChallenge = record != null && phase == 1 && member && wallet != asText(record['proposer']) && _now < asInt(record['challenge_end']);
    final canVote = record != null && phase == 2 && member;
    final canFinalize = record != null && ((phase == 1 && _now >= asInt(record['challenge_end'])) || (phase == 2 && (asInt(record['votes_proposal']) >= asInt(record['m']) || asInt(record['votes_challenge']) >= asInt(record['m']) || _now >= asInt(record['vote_end']))) || (phase == 0 && _now >= graceEnd));
    final canVoid = record != null && phase == 0 && !terminal && member && status == 2 && _now < close;
    final canHalt = member && record != null && !terminal && _now < close;
    final failed = phase == 4;
    final voided = phase == 5;
    final family = _family;
    return Scaffold(
      appBar: AppBar(title: Text(widget.title)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text(record == null ? '尚未开窗' : asText(record['phase_name'], '决议'), style: const TextStyle(color: amber, fontSize: 18)),
          const SizedBox(height: 8),
          Text('家族 $family · 状态 ${statusName(status)}'),
          Text('报告截止 $deadline · 挑战结束 ${asInt(record?['challenge_end'])}'),
          const SizedBox(height: 12),
          if (family == 0) ...[
            TextField(controller: _value, decoration: const InputDecoration(labelText: '主队')),
            TextField(controller: _away, decoration: const InputDecoration(labelText: '客队')),
          ] else if (family == 4)
            TextField(controller: _value, decoration: const InputDecoration(labelText: 'Bernoulli 结果，0 或 1')),
          if (family == 1 || family == 2)
            TextField(controller: _value, decoration: const InputDecoration(labelText: '结果')),
          if (family == 3) TextField(controller: _value, keyboardType: TextInputType.number, decoration: const InputDecoration(labelText: '结果格')),
          TextField(controller: _evidence, decoration: const InputDecoration(labelText: '证据，64 位 hex 或一段说明')),
          const SizedBox(height: 12),
          DeskMessage(_note),
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              if (canOpen) FilledButton(onPressed: _busy ? null : () => _run('resolve_open'), child: const Text('开窗')),
              if (canLock) OutlinedButton(onPressed: _busy ? null : () => _run('lock_committee_bond'), child: const Text('锁定保证金')),
              if (canSubmit) FilledButton(onPressed: _busy ? null : () => _run('submit_result'), child: const Text('提交结果')),
              if (canHalt) OutlinedButton(onPressed: _busy ? null : () => _run('halt'), child: const Text('停牌')),
              if (canChallenge) FilledButton(onPressed: _busy ? null : () => _run('challenge'), child: const Text('挑战')),
              if (canVote) OutlinedButton(onPressed: _busy ? null : () => _run('vote', {'for_challenge': false}), child: const Text('维持原案')),
              if (canVote) FilledButton(onPressed: _busy ? null : () => _run('vote', {'for_challenge': true}), child: const Text('支持挑战')),
              if (canFinalize) OutlinedButton(onPressed: _busy ? null : () => _run('finalize'), child: const Text('最终确认')),
              if (canVoid) OutlinedButton(onPressed: _busy ? null : () => _run('void_resolution'), child: const Text('作废')),
              if (isAdmin && phase == 0 && _now >= deadline && _now < graceEnd) ...[
                FilledButton(onPressed: _busy ? null : () => _run('admin_submit_result'), child: const Text('平台提交')),
                OutlinedButton(onPressed: _busy ? null : () => _run('admin_void_resolution'), child: const Text('平台作废')),
              ],
              if (record != null && record['has_final'] == true)
                FilledButton(onPressed: _busy ? null : () => _run('begin_settle'), child: const Text('锁定结算')),
              if ((failed || voided || record?['refunds_due'] == true) && record?['has_final'] != true)
                OutlinedButton(onPressed: _busy ? null : () => _run('begin_refund'), child: const Text('打开退款')),
            ],
          ),
          if (member && record != null && !terminal && _now < close && status != 2)
            const Padding(padding: EdgeInsets.only(top: 12), child: Text('先停牌，委员会才能作废。交易还开着时，作废会被拒绝。')),
        ],
      ),
    );
  }
}
