# THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!!

# Copyright 2020-2023 Tauri Programme within The Commons Conservancy
# SPDX-License-Identifier: Apache-2.0
# SPDX-License-Identifier: MIT

-keep class kr.devfive.hanbeon.* {
  native <methods>;
}

-keep class kr.devfive.hanbeon.WryActivity {
  public <init>(...);

  void setWebView(kr.devfive.hanbeon.RustWebView);
  java.lang.Class getAppClass(...);
  int getId();
  java.lang.String getVersion();
  int startActivity(...);
}

-keep class kr.devfive.hanbeon.Ipc {
  public <init>(...);

  @android.webkit.JavascriptInterface public <methods>;
}

-keep class kr.devfive.hanbeon.RustWebView {
  public <init>(...);

  void loadUrlMainThread(...);
  void loadHTMLMainThread(...);
  void evalScript(...);
}

-keep class kr.devfive.hanbeon.RustWebChromeClient,kr.devfive.hanbeon.RustWebViewClient {
  public <init>(...);
}
