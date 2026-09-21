<script>import icon from './mcp.svg'; let { size = 16 } = $props()</script>
<span class="mcp-icon" style:width={`${size}px`} style:height={`${size}px`} style:mask-image={`url(${icon})`} aria-hidden="true"></span>
<style>.mcp-icon { display: inline-block; flex: none; background: currentColor; mask-size: contain; mask-repeat: no-repeat; mask-position: center; }</style>
