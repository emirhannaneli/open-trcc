# Changelog

## [0.1.8](https://github.com/emirhannaneli/open-trcc/compare/open-trcc-v0.1.7...open-trcc-v0.1.8) (2026-11-22)


### Features

* **install:** fall back to the official OpenRGB AppImage where no package exists ([ce04fa8](https://github.com/emirhannaneli/open-trcc/commit/ce04fa8f5d11be7389c3d81ec6e903d98385e280))
* **install:** install OpenRGB and load i2c-dev for case sync ([c51ca92](https://github.com/emirhannaneli/open-trcc/commit/c51ca923e87b1595d2c504f1bba031383c54af8b))
* **led:** expose the frame's colour for case sync ([ba614c8](https://github.com/emirhannaneli/open-trcc/commit/ba614c81a1182eaa2dbb1666a8160df709fe0059))
* **openrgb:** case sync thread with server lifecycle ([a7ef38e](https://github.com/emirhannaneli/open-trcc/commit/a7ef38e41daf10895e6d85532250b3cba762f2e7))
* **openrgb:** minimal SDK client ([b3eb2ca](https://github.com/emirhannaneli/open-trcc/commit/b3eb2ca3b31f87ed66052f18f09608fe01b51906))
* sync case lighting with the display through OpenRGB ([46b9333](https://github.com/emirhannaneli/open-trcc/commit/46b9333b3868d468385e09c2a935048fe17ab2bc))
* sync case lighting with the display through OpenRGB ([d77340c](https://github.com/emirhannaneli/open-trcc/commit/d77340c96057aa83ec3263fc7488abb9d6968858))
* **ui:** explain case lighting sync requirements ([6268e7b](https://github.com/emirhannaneli/open-trcc/commit/6268e7b4a55a4fc68c03d5e563960d71c4ecf487))


### Bug Fixes

* **led:** animate effects at 8 fps with TRCC's step sizes ([d1ebc34](https://github.com/emirhannaneli/open-trcc/commit/d1ebc34d30a748e4207f6dbf64dcbb1d8c08ba0b))
* **led:** send LED frames at 20 fps ([346427b](https://github.com/emirhannaneli/open-trcc/commit/346427b474eef917137fb7a744c54afbd58615dc))
* **led:** smooth effect transitions at 20 fps ([3d6c885](https://github.com/emirhannaneli/open-trcc/commit/3d6c8858eaf554566601af82b3d9598a379835a2))
* **openrgb:** bind spawned server to localhost, stop it when nothing to drive ([acfada1](https://github.com/emirhannaneli/open-trcc/commit/acfada18dc08613ac049a190d84bad223ab21008))
* **openrgb:** kill spawned server when its parent dies (PR_SET_PDEATHSIG) ([f30cfae](https://github.com/emirhannaneli/open-trcc/commit/f30cfae6fe515cd9874fd504650113b5afa91dba))
* **openrgb:** non-blocking update check and validated replies ([6c511ba](https://github.com/emirhannaneli/open-trcc/commit/6c511baf5aba33f002e7097b9925993a06668898))
* **openrgb:** reap spawned child on worker panic; gate Linux-only spawn code and tests ([fbaef07](https://github.com/emirhannaneli/open-trcc/commit/fbaef078f50feb44bfab04dd273d1d63bfce182d))

## [0.1.7](https://github.com/emirhannaneli/open-trcc/compare/open-trcc-v0.1.6...open-trcc-v0.1.7) (2026-11-20)


### Bug Fixes

* **autostart:** start with the graphical session on compositors that ignore XDG autostart ([8d29784](https://github.com/emirhannaneli/open-trcc/commit/8d29784a6e86bb3aab23f9ac93c5713d2a1e8aec))
* **autostart:** start with the graphical session on compositors that ignore XDG autostart ([7439310](https://github.com/emirhannaneli/open-trcc/commit/7439310ece643e5cbdb64a424b0dfe05fb548dc7))

## [0.1.6](https://github.com/emirhannaneli/open-trcc/compare/open-trcc-v0.1.5...open-trcc-v0.1.6) (2026-11-19)


### Bug Fixes

* **install:** stop the running app process before updating so the new version starts ([63b9c44](https://github.com/emirhannaneli/open-trcc/commit/63b9c44b7ba76321199481f0afe386dd0abdefab))

## [0.1.5](https://github.com/emirhannaneli/open-trcc/compare/open-trcc-v0.1.4...open-trcc-v0.1.5) (2026-11-19)


### Features

* use the Thermalright square mark as the app and tray icon ([1ab0cee](https://github.com/emirhannaneli/open-trcc/commit/1ab0cee5727f428b4d66357e9465f57db750f5b5))

## [0.1.4](https://github.com/emirhannaneli/open-trcc/compare/open-trcc-v0.1.3...open-trcc-v0.1.4) (2026-11-19)


### Features

* give the app its own icon instead of the Tauri template defaults ([30ed038](https://github.com/emirhannaneli/open-trcc/commit/30ed038e0b9ad117cf8a19e7331e7157d6cb760c))
* start on login, single instance, saved settings, and a Linux installer ([d110724](https://github.com/emirhannaneli/open-trcc/commit/d1107246649be2f6c08b013d52cbe9ff6d9486a3))


### Bug Fixes

* blank the cooler display when the app quits instead of freezing on the last frame ([04ff5c9](https://github.com/emirhannaneli/open-trcc/commit/04ff5c9ca7712dbc1c607fc7b5b1e7362f6b9cb6))

## [0.1.3](https://github.com/emirhannaneli/open-trcc/compare/open-trcc-v0.1.2...open-trcc-v0.1.3) (2026-11-19)


### Features

* keep running in the system tray when the window is closed ([0a5709a](https://github.com/emirhannaneli/open-trcc/commit/0a5709a05466ad3fe392212c2401fa384194eca0))


### Bug Fixes

* blank the display when powered off instead of freezing on the last frame ([4219d06](https://github.com/emirhannaneli/open-trcc/commit/4219d0602076b54a3fea1239d6e45962683cabbf))
* drive digital LED displays with TRCC's real protocol and layout ([0a5709a](https://github.com/emirhannaneli/open-trcc/commit/0a5709a05466ad3fe392212c2401fa384194eca0))
* drop unit-value binding of Components::refresh on Windows/macOS ([8c1c5d1](https://github.com/emirhannaneli/open-trcc/commit/8c1c5d125401266adc5ba82f5b9c54fa5fbdc845))

## [0.1.2](https://github.com/emirhannaneli/open-trcc/compare/open-trcc-v0.1.1...open-trcc-v0.1.2) (2026-11-18)


### Bug Fixes

* unwrap sysinfo Components::new_with_refreshed_list Result on Windows/macOS ([f334122](https://github.com/emirhannaneli/open-trcc/commit/f334122f6ac09a5c7b3a2bfcedb7e8aee2ce59fd))

## [0.1.1](https://github.com/emirhannaneli/open-trcc/compare/open-trcc-v0.1.0...open-trcc-v0.1.1) (2026-11-18)


### Features

* initial release of Open TRCC for Linux ([935fdb0](https://github.com/emirhannaneli/open-trcc/commit/935fdb04a76809b92d9fee744f2c2cb0746bbcb7))
* initial release of Open TRCC for Linux, Windows, and macOS ([50ffe00](https://github.com/emirhannaneli/open-trcc/commit/50ffe00c838273f2598f73a0e4a2577ac636180a))
