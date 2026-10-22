<script lang="ts">
  import { onMount, type Snippet } from 'svelte';
  let { id, root, children, onResize }: { id: string; root: HTMLDivElement | undefined; children: Snippet; onResize: () => void } = $props();
  let element: HTMLDivElement;
  let visible = $state(false);
  let height = $state(160);

  onMount(() => {
    const viewport = root;
    const intersection = new IntersectionObserver(entries => {
      const entry = entries[0];
      if (!entry.isIntersecting && element.contains(document.activeElement)) return;
      const selection = window.getSelection();
      if (!entry.isIntersecting && selection && !selection.isCollapsed && selection.containsNode(element, true)) return;
      visible = entry.isIntersecting;
    }, { root: viewport, rootMargin: '700px 0px' });
    const resize = new ResizeObserver(() => {
      if (!visible) return;
      const next = element.getBoundingClientRect().height;
      if (!next || next === height) return;
      if (viewport) {
        const difference = next - height;
        if (element.getBoundingClientRect().bottom < viewport.getBoundingClientRect().top) viewport.scrollTop += difference;
      }
      height = next;
      onResize();
    });
    intersection.observe(element);
    resize.observe(element);
    return () => { intersection.disconnect(); resize.disconnect(); };
  });
</script>

<div bind:this={element} data-message-id={id} style:min-height={visible ? undefined : `${height}px`}>
  {#if visible}{@render children()}{/if}
</div>
