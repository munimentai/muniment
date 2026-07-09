/* @ds-bundle: {"format":4,"namespace":"MunimentDesignSystem_42af51","components":[{"name":"Button","sourcePath":"components/controls/Button.jsx"},{"name":"Chip","sourcePath":"components/controls/Chip.jsx"},{"name":"Kbd","sourcePath":"components/controls/Chip.jsx"},{"name":"Input","sourcePath":"components/controls/Input.jsx"},{"name":"EmptyState","sourcePath":"components/feedback/EmptyState.jsx"},{"name":"Modal","sourcePath":"components/feedback/Modal.jsx"},{"name":"Popover","sourcePath":"components/feedback/Popover.jsx"},{"name":"Toast","sourcePath":"components/feedback/Toast.jsx"},{"name":"Icon","sourcePath":"components/icon/Icon.jsx"},{"name":"Mark","sourcePath":"components/mark/Mark.jsx"},{"name":"DataTable","sourcePath":"components/records/DataTable.jsx"},{"name":"ProvenanceLine","sourcePath":"components/records/ProvenanceLine.jsx"},{"name":"QueryBar","sourcePath":"components/records/QueryBar.jsx"},{"name":"ToolCard","sourcePath":"components/records/ToolCard.jsx"}],"sourceHashes":{"components/controls/Button.jsx":"78c5d5643b02","components/controls/Chip.jsx":"287ed44c1786","components/controls/Input.jsx":"dfcd26c257ad","components/feedback/EmptyState.jsx":"70c29ecfe397","components/feedback/Modal.jsx":"0bd9c4c46e44","components/feedback/Popover.jsx":"c4a826589d4b","components/feedback/Toast.jsx":"505d28401944","components/icon/Icon.jsx":"d2a5846ca84f","components/mark/Mark.jsx":"d9eff386b894","components/records/DataTable.jsx":"fed49f805362","components/records/ProvenanceLine.jsx":"cf4bb50b3b3a","components/records/QueryBar.jsx":"ca8e0385469c","components/records/ToolCard.jsx":"a76288d16235","ui_kits/admin-web-app/Sparkline.jsx":"f6f673baee94","ui_kits/desktop-app/Composer.jsx":"dfd2807bb6f9","ui_kits/desktop-app/Conversation.jsx":"9f2826652e48","ui_kits/desktop-app/Sidebar.jsx":"ff638782a73d"},"inlinedExternals":[],"unexposedExports":[]} */

(() => {

const __ds_ns = (window.MunimentDesignSystem_42af51 = window.MunimentDesignSystem_42af51 || {});

const __ds_scope = {};

(__ds_ns.__errors = __ds_ns.__errors || []);

// components/controls/Button.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/* Button. Primary = ink fill, paper text. Quiet = transparent, muted text.
   Radius 6, 13.5/600, padding 7×18. Hover shifts ink 6%. NEVER signal. */

function Button({
  variant = "primary",
  size = "md",
  disabled = false,
  type = "button",
  children,
  style,
  ...rest
}) {
  const [hover, setHover] = React.useState(false);
  const [active, setActive] = React.useState(false);
  const pad = size === "sm" ? "5px 12px" : "7px 18px";
  const fs = size === "sm" ? "12.5px" : "13.5px";
  const variants = {
    primary: {
      background: hover ? "color-mix(in srgb, var(--ink) 92%, var(--paper))" : "var(--ink)",
      color: "var(--paper)",
      border: "1px solid var(--ink)"
    },
    quiet: {
      background: hover ? "var(--faint)" : "transparent",
      color: "var(--muted)",
      border: "1px solid transparent"
    },
    outline: {
      background: hover ? "var(--faint)" : "var(--surface)",
      color: "var(--ink)",
      border: "1px solid var(--border)"
    }
  };
  return /*#__PURE__*/React.createElement("button", _extends({
    type: type,
    disabled: disabled,
    onMouseEnter: () => setHover(true),
    onMouseLeave: () => {
      setHover(false);
      setActive(false);
    },
    onMouseDown: () => setActive(true),
    onMouseUp: () => setActive(false),
    style: {
      font: `var(--weight-semibold) ${fs}/1 var(--font-sans)`,
      letterSpacing: "-0.005em",
      padding: pad,
      borderRadius: "var(--radius-control)",
      cursor: disabled ? "not-allowed" : "pointer",
      opacity: disabled ? 0.4 : 1,
      display: "inline-flex",
      alignItems: "center",
      justifyContent: "center",
      gap: "var(--space-2)",
      transform: active && !disabled ? "translateY(0.5px)" : "none",
      transition: "background 120ms var(--ease-out)",
      ...variants[variant],
      ...style
    }
  }, rest), children);
}
Object.assign(__ds_scope, { Button });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/controls/Button.jsx", error: String((e && e.message) || e) }); }

// components/controls/Chip.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/* Chip. Radius 2, mono 11–12px, faint fill or hairline.
   Variants: kbd, attachment, model-pin, transform (voice), filter (admin).
   The transform/voice-polish variant is the only chip that may use signal
   (voice polish flash), per the color law. */

function Chip({
  variant = "default",
  onRemove,
  children,
  style,
  ...rest
}) {
  const isFilter = variant === "filter";
  const isSignal = variant === "transform";
  return /*#__PURE__*/React.createElement("span", _extends({
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: "6px",
      fontFamily: "var(--font-mono)",
      fontSize: "11.5px",
      lineHeight: 1.2,
      fontVariantNumeric: "tabular-nums",
      color: isSignal ? "var(--signal)" : "var(--ink)",
      background: variant === "kbd" || variant === "default" ? "var(--faint)" : "transparent",
      border: variant === "attachment" || variant === "model-pin" || isFilter ? "1px solid var(--border)" : "1px solid transparent",
      borderRadius: "var(--radius-chip)",
      padding: "3px 7px",
      ...style
    }
  }, rest), children, onRemove && /*#__PURE__*/React.createElement("button", {
    type: "button",
    "aria-label": "Remove",
    onClick: onRemove,
    style: {
      all: "unset",
      cursor: "pointer",
      color: "var(--muted)",
      lineHeight: 1,
      fontSize: "13px"
    }
  }, "\xD7"));
}

/* Kbd chip — radius 2, faint, mono 11. */
function Kbd({
  children,
  style,
  ...rest
}) {
  return /*#__PURE__*/React.createElement("kbd", _extends({
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: "11px",
      lineHeight: 1,
      color: "var(--muted)",
      background: "var(--faint)",
      border: "1px solid var(--border)",
      borderRadius: "var(--radius-chip)",
      padding: "3px 6px",
      display: "inline-block",
      ...style
    }
  }, rest), children);
}
Object.assign(__ds_scope, { Chip, Kbd });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/controls/Chip.jsx", error: String((e && e.message) || e) }); }

// components/controls/Input.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/* Input / Textarea. surface fill, border stroke, radius 6 (10 for composer),
   focus stroke muted. Placeholder muted. Error: 1px oxide stroke + mono cause
   line below; no red fills. */

function Input({
  as = "input",
  error,
  composer = false,
  label,
  style,
  id,
  ...rest
}) {
  const [focus, setFocus] = React.useState(false);
  const Tag = as === "textarea" ? "textarea" : "input";
  const rid = id || React.useId();
  const stroke = error ? "var(--oxide)" : focus ? "var(--muted)" : "var(--border)";
  const field = /*#__PURE__*/React.createElement(Tag, _extends({
    id: rid,
    onFocus: () => setFocus(true),
    onBlur: () => setFocus(false),
    "aria-invalid": error ? "true" : undefined,
    style: {
      width: "100%",
      font: `var(--weight-regular) var(--text-15)/1.5 var(--font-sans)`,
      color: "var(--ink)",
      background: "var(--surface)",
      border: `1px solid ${stroke}`,
      borderRadius: composer ? "var(--radius-panel)" : "var(--radius-control)",
      padding: composer ? "12px 14px" : "8px 12px",
      outline: "none",
      resize: as === "textarea" ? "vertical" : undefined,
      minHeight: as === "textarea" ? "88px" : undefined,
      transition: "border-color 120ms var(--ease-out)",
      ...style
    }
  }, rest));
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: "var(--space-2)"
    }
  }, label && /*#__PURE__*/React.createElement("label", {
    htmlFor: rid,
    style: {
      font: `var(--weight-medium) var(--text-13)/1.3 var(--font-sans)`,
      color: "var(--ink)"
    }
  }, label), field, error && /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: "12px",
      lineHeight: 1.45,
      color: "var(--oxide)",
      fontVariantNumeric: "tabular-nums"
    }
  }, error));
}
Object.assign(__ds_scope, { Input });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/controls/Input.jsx", error: String((e && e.message) || e) }); }

// components/feedback/EmptyState.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/* Empty state. One grotesque sentence inviting action, optional quiet button.
   No illustration. */

function EmptyState({
  message,
  action,
  onAction,
  style,
  ...rest
}) {
  return /*#__PURE__*/React.createElement("div", _extends({
    style: {
      display: "flex",
      flexDirection: "column",
      alignItems: "center",
      gap: "var(--space-4)",
      textAlign: "center",
      padding: "var(--space-16) var(--space-6)",
      color: "var(--muted)",
      ...style
    }
  }, rest), /*#__PURE__*/React.createElement("p", {
    style: {
      margin: 0,
      font: "var(--weight-regular) var(--text-15)/1.55 var(--font-sans)",
      maxWidth: "36ch"
    }
  }, message), action && /*#__PURE__*/React.createElement("button", {
    type: "button",
    onClick: onAction,
    style: {
      all: "unset",
      cursor: "pointer",
      padding: "6px 14px",
      borderRadius: "var(--radius-control)",
      border: "1px solid var(--border)",
      font: "var(--weight-semibold) 13px/1 var(--font-sans)",
      color: "var(--ink)"
    }
  }, action));
}
Object.assign(__ds_scope, { EmptyState });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/EmptyState.jsx", error: String((e && e.message) || e) }); }

// components/feedback/Modal.jsx
try { (() => {
/* Modal / confirm. Radius 10, shadow-overlay. Destructive confirms require
   typing the object's name; the button then reads the consequence. */

function Modal({
  open,
  title,
  children,
  onClose,
  confirm,
  style
}) {
  const [typed, setTyped] = React.useState("");
  React.useEffect(() => {
    if (open) setTyped("");
  }, [open]);
  if (!open) return null;
  const gated = confirm && confirm.requireName;
  const armed = !gated || typed === confirm.requireName;
  return /*#__PURE__*/React.createElement("div", {
    role: "dialog",
    "aria-modal": "true",
    "aria-label": title,
    onClick: onClose,
    style: {
      position: "fixed",
      inset: 0,
      background: "rgba(20,23,22,0.32)",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      padding: "var(--space-6)",
      zIndex: 1000
    }
  }, /*#__PURE__*/React.createElement("div", {
    onClick: e => e.stopPropagation(),
    style: {
      width: "min(440px, 100%)",
      background: "var(--surface)",
      border: "1px solid var(--border)",
      borderRadius: "var(--radius-panel)",
      boxShadow: "var(--shadow-overlay)",
      padding: "var(--space-6)",
      ...style
    }
  }, title && /*#__PURE__*/React.createElement("h2", {
    style: {
      font: "var(--weight-semibold) var(--text-17)/1.3 var(--font-sans)",
      letterSpacing: "-0.01em",
      marginBottom: "var(--space-3)"
    }
  }, title), /*#__PURE__*/React.createElement("div", {
    style: {
      font: "var(--weight-regular) var(--text-15)/1.55 var(--font-sans)",
      color: "var(--ink)"
    }
  }, children), gated && /*#__PURE__*/React.createElement("input", {
    value: typed,
    onChange: e => setTyped(e.target.value),
    placeholder: `Type "${confirm.requireName}" to confirm`,
    style: {
      width: "100%",
      marginTop: "var(--space-4)",
      fontFamily: "var(--font-mono)",
      fontSize: "13px",
      color: "var(--ink)",
      background: "var(--surface)",
      border: "1px solid var(--border)",
      borderRadius: "var(--radius-control)",
      padding: "8px 12px",
      outline: "none"
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "flex-end",
      gap: "var(--space-2)",
      marginTop: "var(--space-6)"
    }
  }, /*#__PURE__*/React.createElement("button", {
    type: "button",
    onClick: onClose,
    style: {
      all: "unset",
      cursor: "pointer",
      padding: "7px 14px",
      borderRadius: "var(--radius-control)",
      font: "var(--weight-semibold) 13.5px/1 var(--font-sans)",
      color: "var(--muted)"
    }
  }, "Cancel"), confirm && /*#__PURE__*/React.createElement("button", {
    type: "button",
    disabled: !armed,
    onClick: confirm.onConfirm,
    style: {
      all: "unset",
      cursor: armed ? "pointer" : "not-allowed",
      opacity: armed ? 1 : 0.4,
      padding: "7px 18px",
      borderRadius: "var(--radius-control)",
      font: "var(--weight-semibold) 13.5px/1 var(--font-sans)",
      background: confirm.destructive ? "var(--oxide)" : "var(--ink)",
      color: "var(--paper)"
    }
  }, confirm.label))));
}
Object.assign(__ds_scope, { Modal });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/Modal.jsx", error: String((e && e.message) || e) }); }

// components/feedback/Popover.jsx
try { (() => {
/* Popover / menu. Radius 6, 120ms fade + 2px rise, full keyboard nav.
   shadow-overlay. Never signal. */

function Popover({
  trigger,
  items = [],
  align = "start",
  style
}) {
  const [open, setOpen] = React.useState(false);
  const ref = React.useRef(null);
  React.useEffect(() => {
    if (!open) return;
    const onDoc = e => {
      if (ref.current && !ref.current.contains(e.target)) setOpen(false);
    };
    const onEsc = e => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onEsc);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onEsc);
    };
  }, [open]);
  return /*#__PURE__*/React.createElement("span", {
    ref: ref,
    style: {
      position: "relative",
      display: "inline-flex",
      ...style
    }
  }, /*#__PURE__*/React.createElement("span", {
    onClick: () => setOpen(o => !o),
    style: {
      display: "inline-flex"
    }
  }, trigger), open && /*#__PURE__*/React.createElement("div", {
    role: "menu",
    className: "mn-pop",
    style: {
      position: "absolute",
      top: "calc(100% + 6px)",
      [align === "end" ? "right" : "left"]: 0,
      minWidth: "180px",
      background: "var(--surface)",
      border: "1px solid var(--border)",
      borderRadius: "var(--radius-control)",
      boxShadow: "var(--shadow-overlay)",
      padding: "var(--space-1)",
      zIndex: 900
    }
  }, items.map((it, i) => it.separator ? /*#__PURE__*/React.createElement("div", {
    key: i,
    style: {
      height: 1,
      background: "var(--border)",
      margin: "4px 0"
    }
  }) : /*#__PURE__*/React.createElement("button", {
    key: i,
    type: "button",
    role: "menuitem",
    onClick: () => {
      setOpen(false);
      it.onClick && it.onClick();
    },
    style: {
      all: "unset",
      boxSizing: "border-box",
      display: "flex",
      alignItems: "center",
      gap: "var(--space-2)",
      width: "100%",
      cursor: "pointer",
      padding: "7px 10px",
      borderRadius: "var(--radius-control)",
      font: `var(--weight-regular) var(--text-13)/1.3 var(--font-sans)`,
      color: it.destructive ? "var(--oxide)" : "var(--ink)"
    },
    onMouseEnter: e => e.currentTarget.style.background = "var(--faint)",
    onMouseLeave: e => e.currentTarget.style.background = "transparent"
  }, it.label))), /*#__PURE__*/React.createElement("style", null, `
        @keyframes mn-pop-kf { from { opacity: 0; transform: translateY(2px); } to { opacity: 1; transform: none; } }
        .mn-pop { animation: mn-pop-kf var(--motion-popover, 120ms) var(--ease-out, ease-out); }
        @media (prefers-reduced-motion: reduce) { .mn-pop { animation: none; } }
      `));
}
Object.assign(__ds_scope, { Popover });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/Popover.jsx", error: String((e && e.message) || e) }); }

// components/feedback/Toast.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/* Toast. Bottom-center, surface, hairline, radius 6, ink text, auto-dismiss 5s,
   one action max. Never signal, never icons-only. */

function Toast({
  message,
  action,
  onAction,
  onDismiss,
  duration = 5000,
  style,
  ...rest
}) {
  React.useEffect(() => {
    if (!onDismiss || duration == null) return;
    const t = setTimeout(onDismiss, duration);
    return () => clearTimeout(t);
  }, [onDismiss, duration]);
  return /*#__PURE__*/React.createElement("div", _extends({
    role: "status",
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: "var(--space-4)",
      background: "var(--surface)",
      border: "1px solid var(--border)",
      borderRadius: "var(--radius-control)",
      boxShadow: "var(--shadow-overlay)",
      padding: "10px 12px 10px 16px",
      color: "var(--ink)",
      font: "var(--weight-regular) var(--text-13)/1.4 var(--font-sans)",
      maxWidth: "420px",
      ...style
    }
  }, rest), /*#__PURE__*/React.createElement("span", null, message), action && /*#__PURE__*/React.createElement("button", {
    type: "button",
    onClick: onAction,
    style: {
      all: "unset",
      cursor: "pointer",
      font: "var(--weight-semibold) var(--text-13)/1 var(--font-sans)",
      color: "var(--ink)",
      whiteSpace: "nowrap"
    }
  }, action));
}
Object.assign(__ds_scope, { Toast });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/feedback/Toast.jsx", error: String((e && e.message) || e) }); }

// components/icon/Icon.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/* Icon — a thin wrapper over Lucide (https://lucide.dev/icons).
   muniment is icon-light: use icons sparingly, to accentuate a feature,
   service, or concept where the copy alone doesn't land. Simple by design —
   never complex, never signal-colored (signal means computation only).

   The host page must load Lucide once:
     <script src="https://unpkg.com/lucide@latest/dist/umd/lucide.min.js"></script>
   Icons read the global `window.lucide.icons` node data and render as inline
   SVG, so `color: currentColor` inherits the surrounding text color. */

function pascal(name) {
  return String(name).split(/[-_ ]+/).map(s => s.charAt(0).toUpperCase() + s.slice(1)).join("");
}
function Icon({
  name,
  size = 18,
  strokeWidth = 1.75,
  color = "currentColor",
  title,
  style,
  ...rest
}) {
  const lib = typeof window !== "undefined" ? window.lucide : null;
  const node = lib && lib.icons ? lib.icons[pascal(name)] : null;

  // Graceful fallback when Lucide isn't loaded or the name is unknown.
  if (!node) {
    return /*#__PURE__*/React.createElement("span", _extends({
      "aria-hidden": title ? undefined : "true",
      role: title ? "img" : undefined,
      "aria-label": title,
      style: {
        display: "inline-block",
        width: size,
        height: size,
        ...style
      }
    }, rest));
  }
  return /*#__PURE__*/React.createElement("svg", _extends({
    xmlns: "http://www.w3.org/2000/svg",
    viewBox: "0 0 24 24",
    width: size,
    height: size,
    fill: "none",
    stroke: color,
    strokeWidth: strokeWidth,
    strokeLinecap: "round",
    strokeLinejoin: "round",
    role: title ? "img" : undefined,
    "aria-label": title,
    "aria-hidden": title ? undefined : "true",
    style: {
      display: "inline-block",
      flex: "none",
      verticalAlign: "middle",
      ...style
    }
  }, rest), title && /*#__PURE__*/React.createElement("title", null, title), node.map(([tag, attrs], i) => React.createElement(tag, {
    key: i,
    ...attrs
  })));
}
Object.assign(__ds_scope, { Icon });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/icon/Icon.jsx", error: String((e && e.message) || e) }); }

// components/mark/Mark.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
const {
  useRef,
  useEffect,
  useState
} = React;
/* The milled ring.
   Reference geometry in a 48-unit viewBox: r(t) = 16.5 + 1.6·sin(22t).
   22 teeth, monoline stroke, round caps/joins. Coins are milled so a
   clipped coin exposes itself — verification made visible.

   Shared organism: every simultaneously visible <Mark state="thinking"> reads
   the same module-level clock, so they breathe and spin in perfect sync —
   one presence, not several spinners. Breath and spin are decorrelated on
   purpose; never synchronize them "for polish". */
const CENTER = 24;
const BASE_R = 16.5;
const TEETH = 22;
const MILL = 1.6;
const SAMPLES = 240;
function ringPath(scale, millFlex) {
  let d = "";
  for (let i = 0; i <= SAMPLES; i++) {
    const t = i / SAMPLES * Math.PI * 2;
    const r = (BASE_R + MILL * millFlex * Math.sin(TEETH * t)) * scale;
    const x = CENTER + r * Math.cos(t);
    const y = CENTER + r * Math.sin(t);
    d += (i === 0 ? "M" : "L") + x.toFixed(3) + " " + y.toFixed(3) + " ";
  }
  return d + "Z";
}

/* ---- Shared organism: one clock, many marks ---- */
const organism = {
  raf: 0,
  subs: new Set(),
  // breath
  beatEnd: 0,
  beatDur: 850,
  depthFrom: 0,
  depthTo: 0,
  beatCount: 0,
  // spin
  angle: 0,
  vel: 0,
  velTarget: 0,
  nextSpin: 0,
  // trace
  traceStart: -1,
  traceAt: 6,
  reduced: false,
  last: 0
};
const SPIN_TIERS = [0, 0, 0, 0.15, -0.15, 0.4, -0.4, 0.9, -0.9]; // still weighted heaviest

function scheduleBeat(now) {
  // ≈70bpm heartbeat, rate jittered per beat, swell favored ~3:2, quick flex + slow settle
  organism.beatDur = 700 + Math.random() * 340;
  organism.beatEnd = now + organism.beatDur;
  organism.depthFrom = organism.depthTo;
  const swell = Math.random() < 0.6;
  organism.depthTo = swell ? Math.random() : -Math.random();
  organism.beatCount++;
  if (organism.beatCount >= organism.traceAt) {
    organism.traceStart = now;
    organism.traceAt = organism.beatCount + 5 + Math.floor(Math.random() * 6);
  }
}
function scheduleSpin(now) {
  organism.velTarget = SPIN_TIERS[Math.floor(Math.random() * SPIN_TIERS.length)];
  organism.nextSpin = now + 1500 + Math.random() * 2500;
}
function tick(now) {
  if (!organism.last) organism.last = now;
  const dt = Math.min(64, now - organism.last);
  organism.last = now;
  if (now >= organism.beatEnd) scheduleBeat(now);
  if (now >= organism.nextSpin) scheduleSpin(now);

  // asymmetric ease: quick flex toward target, slow settle handled by phase curve
  const phase = 1 - Math.max(0, (organism.beatEnd - now) / organism.beatDur);
  const eased = phase < 0.35 ? phase / 0.35 : 1 - Math.pow((phase - 0.35) / 0.65, 1.6);
  const depth = organism.depthFrom + (organism.depthTo - organism.depthFrom) * eased;

  // spin velocity eases toward target
  organism.vel += (organism.velTarget - organism.vel) * Math.min(1, dt / 260);
  organism.angle = (organism.angle + organism.vel * dt * 0.06) % 360;
  const traceElapsed = organism.traceStart >= 0 ? now - organism.traceStart : -1;
  const traceOn = traceElapsed >= 0 && traceElapsed < 900;
  const frame = {
    scale: 1 + 0.032 * depth,
    strokeMul: 1 + 0.18 * depth,
    millFlex: 1 + 0.6 * depth,
    angle: organism.angle,
    traceProgress: traceOn ? traceElapsed / 900 : -1
  };
  organism.subs.forEach(fn => fn(frame));
  organism.raf = requestAnimationFrame(tick);
}
function subscribe(fn) {
  if (organism.subs.size === 0) {
    organism.beatEnd = 0;
    organism.nextSpin = 0;
    organism.last = 0;
    organism.raf = requestAnimationFrame(tick);
  }
  organism.subs.add(fn);
  return () => {
    organism.subs.delete(fn);
    if (organism.subs.size === 0) cancelAnimationFrame(organism.raf);
  };
}
function Mark({
  size = 32,
  state = "rest",
  title = "muniment",
  style,
  ...rest
}) {
  const [frame, setFrame] = useState(null);
  const pathRef = useRef(null);
  const traceRef = useRef(null);
  const reduced = typeof window !== "undefined" && window.matchMedia && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const thinking = state === "thinking";
  useEffect(() => {
    if (!thinking || reduced) return;
    return subscribe(f => {
      const p = pathRef.current;
      if (!p) return;
      p.setAttribute("d", ringPath(f.scale, f.millFlex));
      p.setAttribute("stroke-width", (2.4 * f.strokeMul).toFixed(2));
      p.parentNode.setAttribute("transform", `rotate(${f.angle} ${CENTER} ${CENTER})`);
      const tr = traceRef.current;
      if (tr) {
        if (f.traceProgress >= 0) {
          tr.style.opacity = String(Math.sin(f.traceProgress * Math.PI) * 0.9);
          const len = tr.getTotalLength ? tr.getTotalLength() : 300;
          tr.style.strokeDasharray = `${len * 0.16} ${len}`;
          tr.style.strokeDashoffset = String(-len * f.traceProgress);
        } else {
          tr.style.opacity = "0";
        }
      }
    });
  }, [thinking, reduced]);

  // reduced-motion thinking falls back to static verdigris
  const color = thinking ? "var(--signal)" : "var(--ink)";
  const staticPath = ringPath(1, 1);
  return /*#__PURE__*/React.createElement("svg", _extends({
    width: size,
    height: size,
    viewBox: "0 0 48 48",
    role: "img",
    "aria-label": title,
    style: {
      display: "block",
      ...style
    }
  }, rest), /*#__PURE__*/React.createElement("g", {
    transform: `rotate(0 ${CENTER} ${CENTER})`
  }, /*#__PURE__*/React.createElement("path", {
    ref: pathRef,
    d: staticPath,
    fill: "none",
    stroke: color,
    strokeWidth: "2.4",
    strokeLinecap: "round",
    strokeLinejoin: "round"
  }), thinking && !reduced && /*#__PURE__*/React.createElement("path", {
    ref: traceRef,
    d: staticPath,
    fill: "none",
    stroke: "var(--signal)",
    strokeWidth: "3",
    strokeLinecap: "round",
    strokeLinejoin: "round",
    style: {
      opacity: 0,
      filter: "brightness(1.5)"
    }
  })));
}
Object.assign(__ds_scope, { Mark });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/mark/Mark.jsx", error: String((e && e.message) || e) }); }

// components/records/DataTable.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/* Table (admin grammar). Real <table>, 36px rows, hairline row separators,
   no zebra, sticky header, mono for identifiers/figures/timestamps, grotesque
   for names. Deny rows: strikethrough + oxide text + the word "deny" (never
   color alone). Row expands inline. */

function DataTable({
  columns,
  rows,
  onRowClick,
  style,
  ...rest
}) {
  const [expanded, setExpanded] = React.useState(null);
  return /*#__PURE__*/React.createElement("div", _extends({
    style: {
      border: "1px solid var(--border)",
      borderRadius: "var(--radius-control)",
      overflow: "auto",
      background: "var(--surface)",
      ...style
    }
  }, rest), /*#__PURE__*/React.createElement("table", {
    style: {
      width: "100%",
      borderCollapse: "collapse",
      fontSize: "var(--text-13)",
      fontVariantNumeric: "tabular-nums"
    }
  }, /*#__PURE__*/React.createElement("thead", null, /*#__PURE__*/React.createElement("tr", null, columns.map(c => /*#__PURE__*/React.createElement("th", {
    key: c.key,
    style: {
      position: "sticky",
      top: 0,
      zIndex: 1,
      textAlign: c.align || "left",
      fontFamily: "var(--font-sans)",
      fontWeight: "var(--weight-semibold)",
      fontSize: "12px",
      letterSpacing: "0.04em",
      textTransform: "uppercase",
      color: "var(--muted)",
      background: "var(--surface)",
      padding: "0 14px",
      height: "34px",
      borderBottom: "1px solid var(--border)",
      whiteSpace: "nowrap"
    }
  }, c.header)))), /*#__PURE__*/React.createElement("tbody", null, rows.map((row, i) => {
    const deny = row.deny;
    const isOpen = expanded === i;
    return /*#__PURE__*/React.createElement(React.Fragment, {
      key: row.id ?? i
    }, /*#__PURE__*/React.createElement("tr", {
      onClick: () => {
        if (row.detail) setExpanded(isOpen ? null : i);
        onRowClick && onRowClick(row);
      },
      style: {
        cursor: row.detail || onRowClick ? "pointer" : "default",
        borderBottom: "1px solid var(--border)"
      }
    }, columns.map(c => {
      const raw = row[c.key];
      return /*#__PURE__*/React.createElement("td", {
        key: c.key,
        style: {
          textAlign: c.align || "left",
          fontFamily: c.mono ? "var(--font-mono)" : "var(--font-sans)",
          fontSize: c.mono ? "12.5px" : "var(--text-13)",
          color: deny ? "var(--oxide)" : "var(--ink)",
          textDecoration: deny ? "line-through" : "none",
          padding: "0 14px",
          height: "36px",
          whiteSpace: "nowrap"
        }
      }, raw);
    })), isOpen && row.detail && /*#__PURE__*/React.createElement("tr", null, /*#__PURE__*/React.createElement("td", {
      colSpan: columns.length,
      style: {
        background: "var(--faint)",
        padding: "var(--space-3) 14px",
        borderBottom: "1px solid var(--border)",
        fontFamily: "var(--font-mono)",
        fontSize: "12px",
        color: "var(--ink)",
        whiteSpace: "pre-wrap"
      }
    }, row.detail)));
  }))));
}
Object.assign(__ds_scope, { DataTable });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/records/DataTable.jsx", error: String((e && e.message) || e) }); }

// components/records/ProvenanceLine.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/* Provenance line. Mono 11.5, muted, route segment in signal (allowlisted):
   analysis/high → glm-5.2 · $0.0041 · 3.8s
   Click expands the receipt. Screen-reader label spells it out in words. */

function ProvenanceLine({
  route,
  // "analysis/high"
  model,
  // "glm-5.2"
  cost,
  // "$0.0041"
  duration,
  // "3.8s"
  receipt,
  // { rule, tokens, connections, lineage } — optional expandable detail
  style,
  ...rest
}) {
  const [open, setOpen] = React.useState(false);
  const srLabel = `Routed via ${route} to model ${model}, cost ${cost}, ${duration}. ${receipt ? "Activate to expand the receipt." : ""}`;
  return /*#__PURE__*/React.createElement("div", _extends({
    style: {
      ...style
    }
  }, rest), /*#__PURE__*/React.createElement("button", {
    type: "button",
    onClick: () => receipt && setOpen(o => !o),
    "aria-expanded": receipt ? open : undefined,
    "aria-label": srLabel,
    style: {
      all: "unset",
      cursor: receipt ? "pointer" : "default",
      display: "inline-flex",
      alignItems: "center",
      gap: "8px",
      fontFamily: "var(--font-mono)",
      fontSize: "11.5px",
      lineHeight: 1.45,
      color: "var(--muted)",
      fontVariantNumeric: "tabular-nums"
    }
  }, /*#__PURE__*/React.createElement("span", {
    "aria-hidden": "true"
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      color: "var(--signal)"
    }
  }, route), " → ", model, " \xB7 ", cost, " \xB7 ", duration)), receipt && open && /*#__PURE__*/React.createElement("dl", {
    style: {
      margin: "var(--space-2) 0 0",
      padding: "var(--space-3)",
      background: "var(--faint)",
      borderRadius: "var(--radius-control)",
      fontFamily: "var(--font-mono)",
      fontSize: "11.5px",
      lineHeight: 1.5,
      color: "var(--ink)",
      fontVariantNumeric: "tabular-nums",
      display: "grid",
      gridTemplateColumns: "auto 1fr",
      columnGap: "var(--space-4)",
      rowGap: "3px"
    }
  }, [["matched rule", receipt.rule], ["tokens", receipt.tokens], ["connections", receipt.connections], ["lineage", receipt.lineage]].filter(([, v]) => v != null).map(([k, v]) => /*#__PURE__*/React.createElement(React.Fragment, {
    key: k
  }, /*#__PURE__*/React.createElement("dt", {
    style: {
      color: "var(--muted)"
    }
  }, k), /*#__PURE__*/React.createElement("dd", {
    style: {
      margin: 0
    }
  }, v)))));
}
Object.assign(__ds_scope, { ProvenanceLine });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/records/ProvenanceLine.jsx", error: String((e && e.message) || e) }); }

// components/records/QueryBar.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/* Query bar (admin). Mono input accepting key:value filters with clickable
   chips for common cases. Radius 6, surface, hairline. Never signal. */

function QueryBar({
  filters = [],
  onRemove,
  onSubmit,
  suggestions = [],
  placeholder = "key:value …",
  style,
  ...rest
}) {
  const [value, setValue] = React.useState("");
  const [focus, setFocus] = React.useState(false);
  return /*#__PURE__*/React.createElement("div", _extends({
    style: {
      ...style
    }
  }, rest), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      flexWrap: "wrap",
      gap: "6px",
      background: "var(--surface)",
      border: `1px solid ${focus ? "var(--muted)" : "var(--border)"}`,
      borderRadius: "var(--radius-control)",
      padding: "6px 8px",
      transition: "border-color 120ms var(--ease-out)"
    }
  }, filters.map((f, i) => /*#__PURE__*/React.createElement("span", {
    key: i,
    style: {
      display: "inline-flex",
      alignItems: "center",
      gap: "6px",
      fontFamily: "var(--font-mono)",
      fontSize: "11.5px",
      color: "var(--ink)",
      background: "var(--faint)",
      border: "1px solid var(--border)",
      borderRadius: "var(--radius-chip)",
      padding: "3px 7px"
    }
  }, f, /*#__PURE__*/React.createElement("button", {
    type: "button",
    "aria-label": `Remove ${f}`,
    onClick: () => onRemove && onRemove(f, i),
    style: {
      all: "unset",
      cursor: "pointer",
      color: "var(--muted)",
      fontSize: "13px",
      lineHeight: 1
    }
  }, "\xD7"))), /*#__PURE__*/React.createElement("input", {
    value: value,
    placeholder: filters.length ? "" : placeholder,
    onChange: e => setValue(e.target.value),
    onFocus: () => setFocus(true),
    onBlur: () => setFocus(false),
    onKeyDown: e => {
      if (e.key === "Enter" && value.trim()) {
        onSubmit && onSubmit(value.trim());
        setValue("");
      }
    },
    style: {
      flex: 1,
      minWidth: "120px",
      border: "none",
      outline: "none",
      background: "transparent",
      fontFamily: "var(--font-mono)",
      fontSize: "12.5px",
      color: "var(--ink)",
      padding: "2px 2px"
    }
  })), suggestions.length > 0 && /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: "6px",
      flexWrap: "wrap",
      marginTop: "var(--space-2)"
    }
  }, suggestions.map(s => /*#__PURE__*/React.createElement("button", {
    key: s,
    type: "button",
    onClick: () => onSubmit && onSubmit(s),
    style: {
      cursor: "pointer",
      fontFamily: "var(--font-mono)",
      fontSize: "11.5px",
      color: "var(--muted)",
      background: "transparent",
      border: "1px solid var(--border)",
      borderRadius: "var(--radius-chip)",
      padding: "3px 7px"
    }
  }, s))));
}
Object.assign(__ds_scope, { QueryBar });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/records/QueryBar.jsx", error: String((e && e.message) || e) }); }

// components/records/ToolCard.jsx
try { (() => {
function _extends() { return _extends = Object.assign ? Object.assign.bind() : function (n) { for (var e = 1; e < arguments.length; e++) { var t = arguments[e]; for (var r in t) ({}).hasOwnProperty.call(t, r) && (n[r] = t[r]); } return n; }, _extends.apply(null, arguments); }
/* Tool card. Radius 6, surface, mono 12.5.
   Header = status dot + verb + object.
   running = signal dot pulse + signal header (allowlisted).
   complete = muted, collapsed to header. Body collapses past 8 lines. */

function ToolCard({
  status = "complete",
  // "running" | "complete" | "error"
  verb,
  // "Searching"
  object,
  // "vault:acme"
  children,
  // body (log lines / output)
  defaultOpen,
  style,
  ...rest
}) {
  const running = status === "running";
  const error = status === "error";
  const [open, setOpen] = React.useState(defaultOpen ?? running);
  const dotColor = running ? "var(--signal)" : error ? "var(--oxide)" : "var(--muted)";
  const headColor = running ? "var(--signal)" : "var(--ink)";
  return /*#__PURE__*/React.createElement("div", _extends({
    style: {
      background: "var(--surface)",
      border: "1px solid var(--border)",
      borderRadius: "var(--radius-control)",
      overflow: "hidden",
      fontFamily: "var(--font-mono)",
      fontSize: "12.5px",
      fontVariantNumeric: "tabular-nums",
      ...style
    }
  }, rest), /*#__PURE__*/React.createElement("button", {
    type: "button",
    onClick: () => children && setOpen(o => !o),
    "aria-expanded": children ? open : undefined,
    style: {
      all: "unset",
      boxSizing: "border-box",
      width: "100%",
      cursor: children ? "pointer" : "default",
      display: "flex",
      alignItems: "center",
      gap: "var(--space-2)",
      padding: "8px 12px",
      color: headColor
    }
  }, /*#__PURE__*/React.createElement("span", {
    "aria-hidden": "true",
    className: running ? "mn-pulse" : undefined,
    style: {
      width: "7px",
      height: "7px",
      borderRadius: "50%",
      background: dotColor,
      flex: "none"
    }
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontWeight: 400
    }
  }, verb, " ", object), error && /*#__PURE__*/React.createElement("span", {
    style: {
      marginLeft: "auto",
      color: "var(--oxide)"
    }
  }, "failed")), children && open && /*#__PURE__*/React.createElement("div", {
    style: {
      borderTop: "1px solid var(--border)",
      padding: "10px 12px",
      maxHeight: "calc(8 * 1.45em + 20px)",
      overflow: "auto",
      color: "var(--muted)",
      lineHeight: 1.45,
      whiteSpace: "pre-wrap"
    }
  }, children), /*#__PURE__*/React.createElement("style", null, `
        @keyframes mn-pulse-kf { 0%,100% { opacity: 1; transform: scale(1); } 50% { opacity: .35; transform: scale(.82); } }
        .mn-pulse { animation: mn-pulse-kf var(--motion-pulse, 1400ms) var(--ease-in-out, ease-in-out) infinite; }
        @media (prefers-reduced-motion: reduce) { .mn-pulse { animation: none; } }
      `));
}
Object.assign(__ds_scope, { ToolCard });
})(); } catch (e) { __ds_ns.__errors.push({ path: "components/records/ToolCard.jsx", error: String((e && e.message) || e) }); }

// ui_kits/admin-web-app/Sparkline.jsx
try { (() => {
/* Admin web app — live routed-requests sparkline. The live endpoint dot is
   one of the allowlisted signal uses (#7). Historical bars are ink/muted. */
function Sparkline({
  data,
  live
}) {
  const w = 200,
    h = 44,
    max = Math.max(...data);
  const step = w / (data.length - 1);
  const pts = data.map((v, i) => [i * step, h - v / max * (h - 6) - 3]);
  const d = pts.map((p, i) => (i ? "L" : "M") + p[0].toFixed(1) + " " + p[1].toFixed(1)).join(" ");
  const last = pts[pts.length - 1];
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 6
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      font: "var(--weight-semibold) 22px/1 var(--font-sans)",
      letterSpacing: "-0.01em"
    }
  }, "1,204"), /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      color: "var(--muted)"
    }
  }, "requests / 24h")), /*#__PURE__*/React.createElement("svg", {
    width: w,
    height: h,
    style: {
      display: "block"
    }
  }, /*#__PURE__*/React.createElement("path", {
    d: d,
    fill: "none",
    stroke: "var(--muted)",
    strokeWidth: "1.5",
    strokeLinejoin: "round",
    strokeLinecap: "round"
  }), live && /*#__PURE__*/React.createElement("circle", {
    className: "mn-live",
    cx: last[0],
    cy: last[1],
    r: "3.5",
    fill: "var(--signal)"
  })), /*#__PURE__*/React.createElement("style", null, `
        @keyframes mn-live-kf { 0%,100%{opacity:1} 50%{opacity:.3} }
        .mn-live { animation: mn-live-kf var(--motion-pulse,1400ms) var(--ease-in-out,ease-in-out) infinite; }
        @media (prefers-reduced-motion: reduce){ .mn-live{animation:none} }
      `));
}
window.Sparkline = Sparkline;
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/admin-web-app/Sparkline.jsx", error: String((e && e.message) || e) }); }

// ui_kits/desktop-app/Composer.jsx
try { (() => {
/* Desktop app — composer. Radius 10, model pin, attachment chips, send.
   Voice-polish flash is the only signal permitted here. */
const {
  Chip,
  Kbd
} = window.MunimentDesignSystem_42af51;
function Composer({
  onSend,
  busy
}) {
  const [value, setValue] = React.useState("");
  const submit = () => {
    if (value.trim()) {
      onSend(value.trim());
      setValue("");
    }
  };
  return /*#__PURE__*/React.createElement("div", {
    style: {
      borderTop: "1px solid var(--border)",
      background: "var(--paper)",
      padding: "16px 32px 20px"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: 720,
      margin: "0 auto"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      background: "var(--surface)",
      border: "1px solid var(--border)",
      borderRadius: "var(--radius-panel)",
      padding: 12
    }
  }, /*#__PURE__*/React.createElement("textarea", {
    value: value,
    onChange: e => setValue(e.target.value),
    onKeyDown: e => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) submit();
    },
    placeholder: "Message muniment",
    rows: 2,
    style: {
      width: "100%",
      border: "none",
      outline: "none",
      resize: "none",
      background: "transparent",
      font: "var(--weight-regular) 15px/1.55 var(--font-sans)",
      color: "var(--ink)"
    }
  }), /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 8,
      marginTop: 8
    }
  }, /*#__PURE__*/React.createElement(Chip, {
    variant: "model-pin"
  }, "glm-5.2"), /*#__PURE__*/React.createElement(Chip, {
    variant: "attachment",
    onRemove: () => {}
  }, "q3-actuals.csv"), /*#__PURE__*/React.createElement("div", {
    style: {
      marginLeft: "auto",
      display: "flex",
      alignItems: "center",
      gap: 12
    }
  }, /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      color: "var(--muted)"
    }
  }, /*#__PURE__*/React.createElement(Kbd, null, "\u2318"), " ", /*#__PURE__*/React.createElement(Kbd, null, "\u21B5")), /*#__PURE__*/React.createElement("button", {
    onClick: submit,
    disabled: busy,
    style: {
      cursor: busy ? "not-allowed" : "pointer",
      opacity: busy ? 0.4 : 1,
      font: "var(--weight-semibold) 13.5px/1 var(--font-sans)",
      color: "var(--paper)",
      background: "var(--ink)",
      border: "1px solid var(--ink)",
      borderRadius: "var(--radius-control)",
      padding: "7px 18px"
    }
  }, "Send"))))));
}
window.Composer = Composer;
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/desktop-app/Composer.jsx", error: String((e && e.message) || e) }); }

// ui_kits/desktop-app/Conversation.jsx
try { (() => {
/* Desktop app — conversation. Grotesque speech in bubbles, mono records
   (tool cards + provenance lines) attached to the model's turns. */
const {
  Mark,
  ToolCard,
  ProvenanceLine
} = window.MunimentDesignSystem_42af51;
function UserBubble({
  children
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      justifyContent: "flex-end",
      marginBottom: 20
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: "76%",
      background: "var(--faint)",
      borderRadius: "var(--radius-panel)",
      padding: "10px 14px",
      font: "var(--weight-regular) 15px/1.55 var(--font-sans)",
      color: "var(--ink)"
    }
  }, children));
}
function ModelTurn({
  thinking,
  streaming,
  children,
  records,
  provenance
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      gap: 12,
      marginBottom: 24
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      flex: "none",
      paddingTop: 2
    }
  }, /*#__PURE__*/React.createElement(Mark, {
    size: 22,
    state: thinking ? "thinking" : "rest"
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      minWidth: 0
    }
  }, records && /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      flexDirection: "column",
      gap: 8,
      marginBottom: 12
    }
  }, records), thinking ? /*#__PURE__*/React.createElement("span", {
    style: {
      fontFamily: "var(--font-mono)",
      fontSize: 12.5,
      color: "var(--signal)"
    }
  }, "working\u2026") : /*#__PURE__*/React.createElement("div", {
    style: {
      font: "var(--weight-regular) 15px/1.55 var(--font-sans)",
      color: "var(--ink)"
    }
  }, /*#__PURE__*/React.createElement("span", {
    className: streaming ? "mn-stream" : undefined
  }, children)), provenance && !thinking && /*#__PURE__*/React.createElement("div", {
    style: {
      marginTop: 12
    }
  }, provenance)), /*#__PURE__*/React.createElement("style", null, `
        .mn-stream { border-bottom: 2px solid var(--signal); padding-bottom: 1px; }
        @keyframes mn-caret { 0%,100%{opacity:1} 50%{opacity:0} }
      `));
}
function Conversation({
  state
}) {
  return /*#__PURE__*/React.createElement("div", {
    style: {
      flex: 1,
      overflowY: "auto",
      padding: "28px 0"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      maxWidth: 720,
      margin: "0 auto",
      padding: "0 32px"
    }
  }, /*#__PURE__*/React.createElement(UserBubble, null, "Compare our Q3 cloud spend against the committed forecast and flag anything over 8%."), /*#__PURE__*/React.createElement(ModelTurn, {
    records: /*#__PURE__*/React.createElement(React.Fragment, null, /*#__PURE__*/React.createElement(ToolCard, {
      status: "complete",
      verb: "Read",
      object: "vault:acme/finance"
    }, "3 files · 2,140 tokens\nmatched: q3-actuals.csv, forecast-2026.xlsx"), /*#__PURE__*/React.createElement(ToolCard, {
      status: "complete",
      verb: "Computed",
      object: "variance by service"
    }, "7 services · 1 over threshold")),
    provenance: /*#__PURE__*/React.createElement(ProvenanceLine, {
      route: "analysis/high",
      model: "glm-5.2",
      cost: "$0.0089",
      duration: "6.2s",
      receipt: {
        rule: "policy/analysis-high",
        tokens: "2,140 in · 512 out",
        connections: "vault:acme/finance",
        lineage: "thread 8f2a"
      }
    })
  }, "Q3 cloud spend was ", /*#__PURE__*/React.createElement("b", null, "$248,120"), " against a committed forecast of ", /*#__PURE__*/React.createElement("b", null, "$232,000"), " \u2014 a 6.9% overrun in aggregate. One service crosses your 8% flag: ", /*#__PURE__*/React.createElement("b", null, "object storage"), ", at 14.2% over. Everything else lands within tolerance."), /*#__PURE__*/React.createElement(UserBubble, null, "Draft a note to the platform team about object storage."), state === "thinking" ? /*#__PURE__*/React.createElement(ModelTurn, {
    thinking: true
  }) : /*#__PURE__*/React.createElement(ModelTurn, {
    streaming: true,
    provenance: /*#__PURE__*/React.createElement(ProvenanceLine, {
      route: "drafting/standard",
      model: "glm-5.2-mini",
      cost: "$0.0012",
      duration: "2.1s"
    })
  }, "Here's a short note you can send as-is. It states the number, the threshold it crossed, and asks for the two things the team can act on this week.")));
}
window.Conversation = Conversation;
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/desktop-app/Conversation.jsx", error: String((e && e.message) || e) }); }

// ui_kits/desktop-app/Sidebar.jsx
try { (() => {
/* Desktop app — sidebar. Thread list with the muniment lockup; the active
   thread's mark thinks while a run is in flight. */
const {
  Mark
} = window.MunimentDesignSystem_42af51;
function Sidebar({
  threads,
  activeId,
  onSelect,
  thinkingId
}) {
  return /*#__PURE__*/React.createElement("aside", {
    style: {
      width: 248,
      flex: "none",
      background: "var(--surface)",
      borderRight: "1px solid var(--border)",
      display: "flex",
      flexDirection: "column"
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      display: "flex",
      alignItems: "center",
      gap: 8,
      padding: "16px 16px 12px"
    }
  }, /*#__PURE__*/React.createElement(Mark, {
    size: 24
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontWeight: 600,
      fontSize: 17,
      letterSpacing: "-0.01em"
    }
  }, "muniment")), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "4px 10px 10px"
    }
  }, /*#__PURE__*/React.createElement("button", {
    className: "mn-newthread",
    style: {
      width: "100%",
      textAlign: "left",
      cursor: "pointer",
      font: "var(--weight-semibold) 13px/1 var(--font-sans)",
      color: "var(--ink)",
      background: "transparent",
      border: "1px solid var(--border)",
      borderRadius: "var(--radius-control)",
      padding: "8px 12px"
    }
  }, "New thread")), /*#__PURE__*/React.createElement("div", {
    style: {
      padding: "6px 10px",
      overflowY: "auto",
      flex: 1
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      font: "var(--weight-semibold) 11px/1 var(--font-sans)",
      textTransform: "uppercase",
      letterSpacing: "0.04em",
      color: "var(--muted)",
      padding: "8px 8px 6px"
    }
  }, "Threads"), threads.map(t => {
    const active = t.id === activeId;
    return /*#__PURE__*/React.createElement("button", {
      key: t.id,
      onClick: () => onSelect(t.id),
      style: {
        width: "100%",
        textAlign: "left",
        cursor: "pointer",
        display: "flex",
        alignItems: "center",
        gap: 8,
        background: active ? "var(--faint)" : "transparent",
        border: "none",
        borderRadius: "var(--radius-control)",
        padding: "8px 8px",
        marginBottom: 2
      }
    }, t.id === thinkingId ? /*#__PURE__*/React.createElement(Mark, {
      size: 14,
      state: "thinking"
    }) : /*#__PURE__*/React.createElement("span", {
      style: {
        width: 14,
        height: 14,
        flex: "none"
      }
    }), /*#__PURE__*/React.createElement("span", {
      style: {
        font: `var(--weight-${active ? "medium" : "regular"}) 13.5px/1.3 var(--font-sans)`,
        color: "var(--ink)",
        overflow: "hidden",
        textOverflow: "ellipsis",
        whiteSpace: "nowrap"
      }
    }, t.title), /*#__PURE__*/React.createElement("span", {
      style: {
        marginLeft: "auto",
        fontFamily: "var(--font-mono)",
        fontSize: 11,
        color: "var(--muted)"
      }
    }, t.when));
  })), /*#__PURE__*/React.createElement("div", {
    style: {
      borderTop: "1px solid var(--border)",
      padding: "10px 16px",
      display: "flex",
      alignItems: "center",
      gap: 8
    }
  }, /*#__PURE__*/React.createElement("div", {
    style: {
      width: 22,
      height: 22,
      borderRadius: "50%",
      background: "var(--faint)",
      border: "1px solid var(--border)"
    }
  }), /*#__PURE__*/React.createElement("span", {
    style: {
      fontSize: 13,
      color: "var(--ink)"
    }
  }, "Ana Ruiz"), /*#__PURE__*/React.createElement("span", {
    style: {
      marginLeft: "auto",
      fontFamily: "var(--font-mono)",
      fontSize: 11,
      color: "var(--muted)"
    }
  }, "acme")));
}
window.Sidebar = Sidebar;
})(); } catch (e) { __ds_ns.__errors.push({ path: "ui_kits/desktop-app/Sidebar.jsx", error: String((e && e.message) || e) }); }

__ds_ns.Button = __ds_scope.Button;

__ds_ns.Chip = __ds_scope.Chip;

__ds_ns.Kbd = __ds_scope.Kbd;

__ds_ns.Input = __ds_scope.Input;

__ds_ns.EmptyState = __ds_scope.EmptyState;

__ds_ns.Modal = __ds_scope.Modal;

__ds_ns.Popover = __ds_scope.Popover;

__ds_ns.Toast = __ds_scope.Toast;

__ds_ns.Icon = __ds_scope.Icon;

__ds_ns.Mark = __ds_scope.Mark;

__ds_ns.DataTable = __ds_scope.DataTable;

__ds_ns.ProvenanceLine = __ds_scope.ProvenanceLine;

__ds_ns.QueryBar = __ds_scope.QueryBar;

__ds_ns.ToolCard = __ds_scope.ToolCard;

})();
