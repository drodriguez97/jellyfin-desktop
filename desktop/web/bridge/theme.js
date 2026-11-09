// Shell-level styling injected into every page. Kept to plain CSS with no
// filters or animations so it costs nothing at paint time:
//  - slim dark scrollbars instead of the stock desktop ones
//  - no text selection highlight flashes on the TV-style UI
(function () {
    const css = `
        html { scrollbar-width: thin; scrollbar-color: rgba(255,255,255,0.28) transparent; }
        ::-webkit-scrollbar { width: 8px; height: 8px; }
        ::-webkit-scrollbar-track { background: transparent; }
        ::-webkit-scrollbar-thumb { background: rgba(255,255,255,0.28); border-radius: 4px; }
        ::-webkit-scrollbar-thumb:hover { background: rgba(255,255,255,0.45); }
        ::-webkit-scrollbar-corner { background: transparent; }
    `;
    const inject = () => {
        if (document.getElementById('jfd-theme')) return;
        const style = document.createElement('style');
        style.id = 'jfd-theme';
        style.textContent = css;
        (document.head || document.documentElement).appendChild(style);
    };
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', inject, { once: true });
    } else {
        inject();
    }
})();
