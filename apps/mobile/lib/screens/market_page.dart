import 'package:flutter/material.dart';

import '../solana/codec.dart';
import '../solana/rules.dart';
import '../ui/desk_theme.dart';
import 'resolve_page.dart';
import 'shell.dart';

class MarketPage extends StatefulWidget {
  const MarketPage({required this.market, required this.title, super.key});

  final String market;
  final String title;

  @override
  State<MarketPage> createState() => _MarketPageState();
}

class _MarketPageState extends State<MarketPage> {
  Map<String, dynamic>? _info;
  List<bool> _cells = const [];
  var _shares = 1;
  var _nonce = 1;
  String _quote = '';
  String _note = '';
  var _busy = false;

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
      final info = await DeskScope.of(context).api.info(widget.market);
      final n = asInt(info['n']);
      if (mounted) {
        setState(() {
          _info = info;
          _cells = List<bool>.filled(n.clamp(0, 64), false);
          if (_cells.isNotEmpty) _cells[0] = true;
        });
      }
      await _syncNonce();
    } catch (e) {
      if (mounted) setState(() => _note = '$e');
    }
  }

  bool get _closed {
    final info = _info;
    if (info == null) return true;
    final settled = asInt(info['board_phase']) >= 1;
    final halted = asInt(info['status']) == 2;
    final close = asInt(info['close_ts']);
    final now = DateTime.now().millisecondsSinceEpoch ~/ 1000;
    return settled || halted || (close > 0 && now >= close);
  }

  String get _mask {
    final n = _info == null ? _cells.length : asInt(_info!['n'], _cells.length);
    return hexMaskForSelection(_cells, n);
  }

  Future<void> _syncNonce() async {
    if (!mounted) return;
    final wallet = DeskScope.of(context);
    if (!wallet.connected) return;
    try {
      final choice = await wallet.chooseNonce(widget.market);
      if (!mounted) return;
      setState(() {
        _nonce = choice.nonce;
        if (choice.replay) {
          _note = 'nonce ${choice.chainLast} 已经成交。下一笔用 ${choice.nonce}，这不是一笔新的买入。';
        }
      });
    } catch (e) {
      if (mounted) setState(() => _note = '$e');
    }
  }

  Future<void> _preview() async {
    setState(() => _busy = true);
    try {
      final quote = await DeskScope.of(context).api.quote(widget.market, _mask, _shares);
      final pay = asInt(quote['pay_usdc']);
      final face = asInt(quote['face_usdc'], _shares);
      final fee = asInt(quote['fee_usdc']);
      setState(() => _quote = '支付 $pay USDC（手续费 $fee）。若命中，面值 $face USDC，未命中为 0。ρ̂ 只表示覆盖，不是赔付。');
    } catch (e) {
      setState(() => _note = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _buy() async {
    final wallet = DeskScope.of(context);
    if (!wallet.connected) {
      setState(() => _note = '先连接钱包');
      return;
    }
    if (_closed) {
      setState(() => _note = '已停牌、已过截止或已结算，不能买入');
      return;
    }
    if (!_cells.contains(true)) {
      setState(() => _note = '先选择区间');
      return;
    }
    setState(() {
      _busy = true;
      _note = '正在签名…';
    });
    try {
      final choice = await wallet.chooseNonce(widget.market);
      if (choice.replay) {
        setState(() {
          _nonce = choice.nonce;
          _note = 'nonce ${choice.chainLast} 已经成交。下一笔用 ${choice.nonce}，这不是一笔新的买入。';
        });
        return;
      }
      final sig = await wallet.submit({
        'op': 'buy_set',
        'owner': wallet.address,
        'trader': wallet.address,
        'market': widget.market,
        'mask': _mask,
        'shares': _shares,
        'nonce': choice.nonce,
      });
      final after = await wallet.rpc.fillNonceLast(wallet.address!, widget.market);
      if (!nonceAdvanced(before: choice.chainLast, after: after)) {
        setState(() {
          _nonce = choice.nonce;
          _note = '链上 nonce 仍是 ${choice.chainLast}。$sig 没有新的成交。';
        });
        return;
      }
      await wallet.rememberNonce(widget.market, after + 1);
      setState(() {
        _note = '已确认 $sig';
        _nonce = after + 1;
      });
    } catch (e) {
      setState(() => _note = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final info = _info;
    final status = info == null ? '' : statusName(asInt(info['status']));
    return Scaffold(
      appBar: AppBar(title: Text(widget.title)),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          Text(status, style: const TextStyle(color: amber)),
          const SizedBox(height: 8),
          Text(shortKey(widget.market), style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
          const SizedBox(height: 16),
          if (_cells.isEmpty) const Text('正在读取盘口…') else const Text('选择要买入的格子'),
          const SizedBox(height: 8),
          Wrap(
            spacing: 6,
            runSpacing: 6,
            children: [
              for (var i = 0; i < _cells.length; i++)
                FilterChip(
                  label: Text('$i'),
                  selected: _cells[i],
                  onSelected: (on) => setState(() => _cells[i] = on),
                ),
            ],
          ),
          if ((info == null ? 0 : asInt(info['n'])) > 64)
            const Padding(
              padding: EdgeInsets.only(top: 8),
              child: Text('这个市场超过 64 格。这里只能勾选前 64 格，其余格子不买入。掩码会补到链上要求的长度。'),
            ),
          const SizedBox(height: 12),
          Row(
            children: [
              const Text('份额'),
              IconButton(
                onPressed: _shares > 1 ? () => setState(() => _shares -= 1) : null,
                icon: const Icon(Icons.remove),
              ),
              Text('$_shares'),
              IconButton(onPressed: () => setState(() => _shares += 1), icon: const Icon(Icons.add)),
              const Spacer(),
              Text('nonce $_nonce'),
            ],
          ),
          DeskMessage(_quote),
          DeskMessage(_note),
          Row(
            children: [
              OutlinedButton(onPressed: _busy ? null : _preview, child: const Text('报价')),
              const SizedBox(width: 8),
              FilledButton(onPressed: _busy || _closed ? null : _buy, child: Text(_closed ? '已停止买入' : '买入')),
            ],
          ),
          const SizedBox(height: 16),
          TextButton(
            onPressed: () {
              Navigator.of(context).push(
                MaterialPageRoute<void>(builder: (_) => ResolvePage(market: widget.market, title: widget.title)),
              );
            },
            child: const Text('决议与结算'),
          ),
        ],
      ),
    );
  }
}
