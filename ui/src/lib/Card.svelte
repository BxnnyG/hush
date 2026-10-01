<script lang="ts">
  import type { Snippet } from 'svelte';
  let { title, sub, dim = false, control, children }: { title: string; sub?: string; dim?: boolean; control?: Snippet; children?: Snippet } = $props();
</script>

<section class="card" class:dim>
  <header>
    <div class="text">
      <h2>{title}</h2>
      {#if sub}<p>{sub}</p>{/if}
    </div>
    {#if control}<div class="control">{@render control()}</div>{/if}
  </header>
  {#if children}<div class="body">{@render children()}</div>{/if}
</section>

<style>
  .card {
    background: var(--card);
    border-radius: var(--radius);
    padding: 16px 18px;
    box-shadow: var(--shadow);
    transition: opacity 0.2s;
  }
  .card.dim {
    opacity: 0.6;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  h2 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
  }
  p {
    margin: 2px 0 0;
    font-size: 13px;
    color: var(--sub);
  }
  .body {
    margin-top: 14px;
  }
</style>
