<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { HeatmapCell } from "./types";

  let cells: HeatmapCell[] = $state([]);
  let loading = $state(true);
  let collapsed = $state(false);

  $effect(() => {
    invoke<HeatmapCell[]>("get_activity_heatmap")
      .then((result) => {
        cells = result;
        loading = false;
      })
      .catch(() => {
        loading = false;
      });
  });

  // 0 = Sunday, but display starts on Monday for a more natural week layout
  const dayOrder = [1, 2, 3, 4, 5, 6, 0];
  const dayLabels = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

  // Lookup grid: byDay[day][hour] = count
  let byDay = $derived.by(() => {
    const grid: number[][] = Array.from({ length: 7 }, () => Array(24).fill(0));
    for (const cell of cells) grid[cell.day][cell.hour] = cell.count;
    return grid;
  });

  let maxCount = $derived(cells.reduce((max, cell) => Math.max(max, cell.count), 0));
  let totalCount = $derived(cells.reduce((sum, cell) => sum + cell.count, 0));

  // Find the busiest hour and busiest day for the summary line
  let peakHour = $derived.by(() => {
    const hourTotals = Array(24).fill(0);
    for (const cell of cells) hourTotals[cell.hour] += cell.count;
    const max = Math.max(...hourTotals);
    if (max === 0) return null;
    const hour = hourTotals.indexOf(max);
    return { hour, count: max };
  });

  let peakDay = $derived.by(() => {
    const dayTotals = Array(7).fill(0);
    for (const cell of cells) dayTotals[cell.day] += cell.count;
    const max = Math.max(...dayTotals);
    if (max === 0) return null;
    const dayIndex = dayTotals.indexOf(max);
    const labels = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    return { day: labels[dayIndex], count: max };
  });

  // Color scale: 0 → background, then 4 quartiles of indigo intensity
  function cellColor(count: number): string {
    if (count === 0) return "#171728";
    const ratio = maxCount > 0 ? count / maxCount : 0;
    // Boost low counts so a single session is visible against the empty background
    const boosted = Math.max(0.18, ratio);
    const alpha = Math.min(0.95, boosted);
    return `rgba(99, 102, 241, ${alpha.toFixed(2)})`;
  }

  function formatHour(hour: number): string {
    if (hour === 0) return "12a";
    if (hour === 12) return "12p";
    if (hour < 12) return `${hour}a`;
    return `${hour - 12}p`;
  }

  function tooltipFor(day: number, hour: number, count: number): string {
    const labels = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    const dayLabel = labels[day];
    const hourLabel = formatHour(hour);
    if (count === 0) return `${dayLabel} ${hourLabel} — no sessions`;
    return `${dayLabel} ${hourLabel} — ${count} session${count === 1 ? "" : "s"}`;
  }
</script>

<section class="heatmap-section">
  <header class="heatmap-header">
    <button class="heatmap-toggle" onclick={() => (collapsed = !collapsed)}>
      <svg
        class="chevron"
        class:collapsed
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2.5"
      >
        <path d="m6 9 6 6 6-6"/>
      </svg>
      <span class="heatmap-title">When you use Claude Code</span>
    </button>
    {#if !loading && totalCount > 0}
      <div class="heatmap-summary">
        {#if peakDay}<span>Busiest day: <strong>{peakDay.day}</strong></span>{/if}
        {#if peakHour}<span>Peak hour: <strong>{formatHour(peakHour.hour)}</strong></span>{/if}
        <span>{totalCount} sessions tracked</span>
      </div>
    {/if}
  </header>

  {#if !collapsed}
    {#if loading}
      <div class="heatmap-status">Loading…</div>
    {:else if totalCount === 0}
      <div class="heatmap-status">No activity to chart yet.</div>
    {:else}
      <div class="heatmap-wrapper">
        <div class="heatmap-grid">
          <!-- Hour labels row -->
          <div class="hour-labels">
            <div class="day-spacer"></div>
            {#each Array(24) as _, hour}
              <div class="hour-label" class:hour-label-shown={hour % 3 === 0}>
                {hour % 3 === 0 ? formatHour(hour) : ""}
              </div>
            {/each}
          </div>

          {#each dayOrder as dayIndex, rowIndex}
            <div class="heatmap-row">
              <div class="day-label">{dayLabels[rowIndex]}</div>
              {#each Array(24) as _, hour}
                <div
                  class="cell"
                  style="background-color: {cellColor(byDay[dayIndex][hour])};"
                  title={tooltipFor(dayIndex, hour, byDay[dayIndex][hour])}
                ></div>
              {/each}
            </div>
          {/each}
        </div>

        <div class="heatmap-legend">
          <span class="legend-text">Less</span>
          <div class="legend-cells">
            <div class="cell" style="background-color: {cellColor(0)};"></div>
            <div class="cell" style="background-color: {cellColor(Math.max(1, maxCount * 0.25))};"></div>
            <div class="cell" style="background-color: {cellColor(Math.max(1, maxCount * 0.5))};"></div>
            <div class="cell" style="background-color: {cellColor(Math.max(1, maxCount * 0.75))};"></div>
            <div class="cell" style="background-color: {cellColor(maxCount)};"></div>
          </div>
          <span class="legend-text">More</span>
        </div>
      </div>
    {/if}
  {/if}
</section>

<style>
  .heatmap-section {
    margin-top: 32px;
    padding: 18px 20px 16px;
    background: #14142a;
    border: 1px solid #1e1e36;
    border-radius: 12px;
  }

  .heatmap-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }

  .heatmap-toggle {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    background: transparent;
    border: none;
    color: #c0c0d8;
    cursor: pointer;
    padding: 0;
    font-family: inherit;
  }

  .chevron {
    color: #818cf8;
    transition: transform 0.15s;
  }

  .chevron.collapsed {
    transform: rotate(-90deg);
  }

  .heatmap-title {
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: #d8d8f0;
  }

  .heatmap-summary {
    display: flex;
    align-items: center;
    gap: 16px;
    font-size: 12px;
    color: #7a7a9a;
    flex-wrap: wrap;
  }

  .heatmap-summary strong {
    color: #c7d2fe;
    font-weight: 600;
  }

  .heatmap-status {
    padding: 24px 0 8px;
    font-size: 12px;
    color: #5a5a7a;
  }

  .heatmap-wrapper {
    margin-top: 14px;
    overflow-x: auto;
  }

  .heatmap-grid {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 520px;
  }

  .hour-labels {
    display: grid;
    grid-template-columns: 36px repeat(24, 1fr);
    gap: 3px;
    margin-bottom: 4px;
  }

  .day-spacer {
    /* Aligns with the day-label column */
  }

  .hour-label {
    font-size: 9px;
    color: #5a5a7a;
    text-align: left;
    line-height: 1;
    height: 10px;
  }

  .hour-label-shown {
    color: #6a6a8a;
  }

  .heatmap-row {
    display: grid;
    grid-template-columns: 36px repeat(24, 1fr);
    gap: 3px;
    align-items: center;
  }

  .day-label {
    font-size: 10px;
    color: #6a6a8a;
    font-weight: 500;
    line-height: 1;
  }

  .cell {
    width: 100%;
    aspect-ratio: 1 / 1;
    max-height: 16px;
    min-height: 12px;
    border-radius: 2px;
    transition: transform 0.1s;
  }

  .cell:hover {
    transform: scale(1.4);
    outline: 1px solid rgba(255, 255, 255, 0.4);
  }

  .heatmap-legend {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 12px;
  }

  .legend-text {
    font-size: 10px;
    color: #6a6a8a;
  }

  .legend-cells {
    display: flex;
    gap: 3px;
  }

  .legend-cells .cell {
    width: 12px;
    height: 12px;
    aspect-ratio: auto;
    min-height: 12px;
  }

  .legend-cells .cell:hover {
    transform: none;
    outline: none;
  }
</style>
