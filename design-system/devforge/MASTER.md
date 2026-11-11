# DevForge desktop visual system

This is a curated product decision, not an unreviewed database-search export.
The two initial design-system queries returned landing-page compositions, which
were rejected for this operational desktop utility. Relevant focus/accessibility
guidance was verified separately; ui-taste's operate/craft guidance owns the layout.

- Target: [generated Settings reference](reference/settings-target.png), 1536×1024.
  The target includes illustrative native titlebar chrome; actual OS chrome is used
  in the app, never painted a second time into the WebView.
- Shell: restrained sidebar, contextual heading, useful working area. Settings is
  the first working surface. Projects explicitly communicates its current limit;
  no invented running services, project data, or decorative dashboards.
- Typography: system UI sans (SF/Segoe UI); monospace only for paths and machine
  identifiers. Fonts work offline and support Cyrillic/Uzbek Latin.
- Light palette: warm-white canvas, soft-grey sidebar, slate text, teal #0F766E.
  Dark palette retains semantic contrast and uses a lighter teal. System/light/dark
  are user choices; system is the initial default, not a forced visual preference.
- Controls: semantic radios/checkboxes, visible focus, consistent Lucide outline
  icons, real saving/error/retry behavior. Selection uses shape/text and ARIA as well
  as color. Motion is small and respects reduced-motion preferences.
- Supported window floor: 820×620; content must remain reachable when text expands.
  Check all three languages, both appearances, and window resizing in the real app.
- UI skills: ui-ux-pro-max for relevant guidance, ui-taste and uizze-ui local
  playbooks for target/implementation, fresh web-design-guidelines for source audit.
  Paid Uizze retrieval was not used or claimed.

Reference generation is a design target, not evidence that the built app matches
or works. Actual screenshots and native/browser checks are recorded under M1 evidence.
