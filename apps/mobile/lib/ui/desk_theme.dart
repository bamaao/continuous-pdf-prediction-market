import 'package:flutter/material.dart';

const ink = Color(0xFF14120E);
const paper = Color(0xFFF4EFE6);
const amber = Color(0xFFE6A15C);
const rule = Color(0xFF3A342C);

ThemeData deskTheme() {
  final base = ThemeData(
    brightness: Brightness.dark,
    scaffoldBackgroundColor: ink,
    colorScheme: const ColorScheme.dark(primary: amber, surface: ink, onSurface: paper),
    dividerColor: rule,
  );
  return base.copyWith(
    appBarTheme: const AppBarTheme(backgroundColor: ink, foregroundColor: paper, elevation: 0),
    navigationBarTheme: NavigationBarThemeData(
      backgroundColor: ink,
      indicatorColor: amber.withValues(alpha: 0.2),
      labelTextStyle: WidgetStateProperty.all(const TextStyle(fontSize: 11)),
    ),
    inputDecorationTheme: const InputDecorationTheme(
      isDense: true,
      border: OutlineInputBorder(),
      labelStyle: TextStyle(color: paper),
    ),
    filledButtonTheme: FilledButtonThemeData(
      style: FilledButton.styleFrom(backgroundColor: amber, foregroundColor: ink),
    ),
  );
}

String shortKey(String? value) {
  if (value == null || value.length < 8) return value ?? '';
  return '${value.substring(0, 4)}…${value.substring(value.length - 4)}';
}

int asInt(Object? value, [int fallback = 0]) {
  if (value is int) return value;
  if (value is num) return value.round();
  return fallback;
}

String asText(Object? value, [String fallback = '']) {
  if (value == null) return fallback;
  return value.toString();
}
