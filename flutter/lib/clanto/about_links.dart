// SPDX-FileCopyrightText: 2026 Clanto Services srls <info@clanto.it>
// SPDX-License-Identifier: AGPL-3.0-only OR Apache-2.0

// Link e titolare del copyright della scheda "About" desktop.
//
// Stanno qui e non in desktop_setting_page.dart perché quella pagina cambia a
// ogni release upstream: nel file upstream resta una sola riga di aggancio.

import 'package:flutter/material.dart';
import 'package:flutter_hbb/clanto/brand.dart';
import 'package:flutter_hbb/common.dart';
import 'package:get/get.dart';
import 'package:url_launcher/url_launcher_string.dart';

const String clantoCopyrightHolder = 'Clanto Services srls';

List<Widget> clantoAboutLinks(TextStyle linkStyle) {
  Widget link(String url, String label) => InkWell(
      onTap: () {
        launchUrlString(url);
      },
      child: Text(
        label,
        style: linkStyle,
      ).marginSymmetric(vertical: 4.0));

  final brand = ClantoBrand.current;
  return [
    link('https://supporto.clanto.it/documentazione/privacy-clantodesk/',
        translate('Privacy Statement')),
    link(brand.websiteUrl, translate('Website')),
    if (brand.branded) link(brand.supportUrl, translate('Support')),
    link('https://github.com/clanto/clantodesk-client',
        'Source code · AGPL-3.0-only'),
    link('https://www.gnu.org/licenses/agpl-3.0.html', 'GNU AGPL v3 license'),
  ];
}
