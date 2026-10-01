<script lang="ts">
  let { checked, onchange, disabled = false, label }: { checked: boolean; onchange: (v: boolean) => void; disabled?: boolean; label: string } = $props();
</script>

<button class="toggle" class:on={checked} role="switch" aria-checked={checked} aria-label={label} {disabled} onclick={() => onchange(!checked)}>
  <span class="knob"></span>
</button>

<style>
  .toggle {
    position: relative;
    width: 52px;
    height: 32px;
    flex: none;
    border: 0;
    border-radius: 16px;
    background: var(--track);
    cursor: pointer;
    transition: background 0.2s ease;
    padding: 0;
  }
  .toggle.on {
    background: var(--accent);
  }
  .toggle:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
  .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 26px;
    height: 26px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.3);
    transition: transform 0.2s cubic-bezier(0.3, 0.7, 0.4, 1);
  }
  .on .knob {
    transform: translateX(20px);
  }
</style>
