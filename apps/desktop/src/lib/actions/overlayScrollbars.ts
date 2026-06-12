/* Custom always-visible scrollbars as a Svelte action.
 *
 * WKWebView ignores `::-webkit-scrollbar` styling whenever macOS runs
 * overlay scrollbars (the trackpad default), so any "always visible"
 * scrollbar in Woom must be its own DOM. Same approach as the editor's
 * `.ed-vbar` (Editor.svelte), generalised: attach to any overflow
 * element and the action appends track + thumb elements to its PARENT,
 * positioned over the scroller's edges. Styles live in app.css
 * (`.wm-sb*`) because action-created nodes can't use scoped styles.
 *
 * Usage: <div class="scrolly" use:overlayScrollbars> — both axes by
 * default; pass { vertical: false } / { horizontal: false } to drop one.
 */

export interface OverlayScrollbarOpts {
  vertical?: boolean;
  horizontal?: boolean;
}

const MIN_THUMB = 20;
const BAR = 8;

export function overlayScrollbars(el: HTMLElement, opts: OverlayScrollbarOpts = {}) {
  const wantV = opts.vertical !== false;
  const wantH = opts.horizontal !== false;
  const parent = el.parentElement;
  if (!parent) return {};
  if (getComputedStyle(parent).position === 'static') {
    parent.style.position = 'relative';
  }

  const mk = (cls: string) => {
    const d = document.createElement('div');
    d.className = cls;
    return d;
  };
  const vTrack = mk('wm-sb wm-sb--v');
  const vThumb = mk('wm-sb-thumb');
  const hTrack = mk('wm-sb wm-sb--h');
  const hThumb = mk('wm-sb-thumb');
  vTrack.appendChild(vThumb);
  hTrack.appendChild(hThumb);
  if (wantV) parent.appendChild(vTrack);
  if (wantH) parent.appendChild(hTrack);

  function refresh() {
    const { scrollTop, scrollLeft, scrollWidth, scrollHeight, clientWidth, clientHeight } = el;
    const top = el.offsetTop;
    const left = el.offsetLeft;
    if (wantV) {
      if (clientHeight === 0 || scrollHeight <= clientHeight + 1) {
        vTrack.style.display = 'none';
      } else {
        vTrack.style.display = '';
        vTrack.style.top = `${top}px`;
        vTrack.style.height = `${clientHeight}px`;
        vTrack.style.left = `${left + clientWidth - BAR}px`;
        const h = Math.max(MIN_THUMB, (clientHeight / scrollHeight) * clientHeight);
        vThumb.style.height = `${h}px`;
        vThumb.style.top = `${(scrollTop / (scrollHeight - clientHeight)) * (clientHeight - h)}px`;
      }
    }
    if (wantH) {
      if (clientWidth === 0 || scrollWidth <= clientWidth + 1) {
        hTrack.style.display = 'none';
      } else {
        hTrack.style.display = '';
        hTrack.style.left = `${left}px`;
        hTrack.style.width = `${clientWidth}px`;
        hTrack.style.top = `${top + clientHeight - BAR}px`;
        const w = Math.max(MIN_THUMB, (clientWidth / scrollWidth) * clientWidth);
        hThumb.style.width = `${w}px`;
        hThumb.style.left = `${(scrollLeft / (scrollWidth - clientWidth)) * (clientWidth - w)}px`;
      }
    }
  }

  function dragThumb(axis: 'v' | 'h') {
    return (e: PointerEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const startPos = axis === 'v' ? e.clientY : e.clientX;
      const startScroll = axis === 'v' ? el.scrollTop : el.scrollLeft;
      const onMove = (ev: PointerEvent) => {
        const { scrollWidth, scrollHeight, clientWidth, clientHeight } = el;
        if (axis === 'v') {
          const denom = clientHeight - vThumb.offsetHeight;
          if (denom <= 0) return;
          el.scrollTop =
            startScroll + (ev.clientY - startPos) * ((scrollHeight - clientHeight) / denom);
        } else {
          const denom = clientWidth - hThumb.offsetWidth;
          if (denom <= 0) return;
          el.scrollLeft =
            startScroll + (ev.clientX - startPos) * ((scrollWidth - clientWidth) / denom);
        }
      };
      const onUp = () => {
        window.removeEventListener('pointermove', onMove);
        window.removeEventListener('pointerup', onUp);
      };
      window.addEventListener('pointermove', onMove);
      window.addEventListener('pointerup', onUp);
    };
  }
  vThumb.addEventListener('pointerdown', dragThumb('v'));
  hThumb.addEventListener('pointerdown', dragThumb('h'));

  /* Track click — jump so the thumb centres on the click point. */
  vTrack.addEventListener('pointerdown', (e) => {
    if (e.target !== vTrack) return;
    const rect = vTrack.getBoundingClientRect();
    const { scrollHeight, clientHeight } = el;
    const denom = clientHeight - vThumb.offsetHeight;
    if (denom <= 0) return;
    const ratio = (e.clientY - rect.top - vThumb.offsetHeight / 2) / denom;
    el.scrollTop = Math.max(0, Math.min(1, ratio)) * (scrollHeight - clientHeight);
  });
  hTrack.addEventListener('pointerdown', (e) => {
    if (e.target !== hTrack) return;
    const rect = hTrack.getBoundingClientRect();
    const { scrollWidth, clientWidth } = el;
    const denom = clientWidth - hThumb.offsetWidth;
    if (denom <= 0) return;
    const ratio = (e.clientX - rect.left - hThumb.offsetWidth / 2) / denom;
    el.scrollLeft = Math.max(0, Math.min(1, ratio)) * (scrollWidth - clientWidth);
  });

  el.addEventListener('scroll', refresh, { passive: true });
  const ro = new ResizeObserver(refresh);
  ro.observe(el);
  /* Content node observed too — it can grow/shrink (streaming diffs,
     details toggle) without the scroller itself resizing. */
  if (el.firstElementChild) ro.observe(el.firstElementChild);
  refresh();

  return {
    destroy() {
      el.removeEventListener('scroll', refresh);
      ro.disconnect();
      vTrack.remove();
      hTrack.remove();
    }
  };
}
