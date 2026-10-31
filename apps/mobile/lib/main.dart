import 'package:flutter/material.dart';

import 'screens/shell.dart';
import 'ui/desk_theme.dart';
import 'wallet/session.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(ContinuousApp(session: WalletSession()));
}

class ContinuousApp extends StatelessWidget {
  const ContinuousApp({required this.session, super.key});

  final WalletSession session;

  @override
  Widget build(BuildContext context) {
    return DeskScope(
      session: session,
      child: MaterialApp(
        title: 'Continuous',
        theme: deskTheme(),
        home: const DeskShell(),
      ),
    );
  }
}
