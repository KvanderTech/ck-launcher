# Kvanth white UI icons

The Windows 11 WebView2 launcher uses the approved blue/cyan family in the compact sidebar and white ribbon-style icons in action buttons, window controls, content and services. The blue K application/sidebar brand remains unchanged. User skin heads and project/build artwork remain untouched. Navigation icons are 26px; action icons are 16–20px. The compact account dropdown is hidden so avatar sizing cannot stretch it.

41 transparent PNG assets were exported from the approved sheets with `scripts/export-approved-icons.cjs`. The search SVG extends the same white style. `KvanthIcon` supplies decorative, aria-hidden assets; surrounding buttons retain their action labels.
Blue sidebar assets are exported from the original approved navigation sheet with `scripts/export-blue-navigation.cjs`.

Build: `npm run tauri:win11:build` in `app`.

Account avatars use the ACTIVE texture from the existing Minecraft Cosmetics profile request, cropping the 8x8 face and hat without smoothing. Changing a skin refreshes the avatar from the shared cosmetics state. Third-party head URLs are used only when no active profile texture is available; failed image loads display a neutral placeholder.
The main play action matches the add-PNG primary button gradient, white weight-800 text, 14px corners and icon placement.
