<script lang="ts">
  // The amber elephant — "elephants never forget" ↔ "never lose a session".
  // Flat fills so it stays crisp at small sizes.
  let { size = 22 }: { size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 32 32"
  fill="none"
  aria-hidden="true"
  class="brand-mark"
>
  <!-- ears -->
  <path d="M9 8.2C5 6.4 2.2 9 2.8 13.2c.5 3.5 3.4 4.8 6 3.6z" fill="#f59e0b" />
  <path d="M23 8.2c4-1.8 6.8.8 6.2 5c-.5 3.5-3.4 4.8-6 3.6z" fill="#f59e0b" />
  <!-- head -->
  <path d="M16 6C10.8 6 8 9.4 8 13.8S10.8 21 16 21s8-2.8 8-7.2S21.2 6 16 6z" fill="#f59e0b" />
  <!-- trunk -->
  <path d="M13.7 18.5h4.6v6.1c0 1.6-1.1 2.5-2.3 2.5s-2.3-.9-2.3-2.5z" fill="#f59e0b" />
  <!-- eyes -->
  <circle cx="12.9" cy="13.3" r="1.2" fill="#241304" />
  <circle cx="19.1" cy="13.3" r="1.2" fill="#241304" />
</svg>

<style>
  .brand-mark {
    display: block;
    flex-shrink: 0;
    filter: drop-shadow(0 1px 3px rgba(245, 158, 11, 0.3));
  }
</style>
