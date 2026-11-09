// Loads poster tiles further ahead and keeps them loaded.
//
// jellyfin-web lazy-loads card images through IntersectionObservers that only
// request an image once its tile is (almost) inside the viewport, so scrolling
// faster than the server answers always shows empty tiles. Two loaders exist:
//  - the React cards use react-lazy-load-image-component, whose shared
//    observers are created with `{ rootMargin: '<threshold>px' }` (100px by
//    default) and keep a tile once it has loaded;
//  - the older cardBuilder views use the in-house lazy loader (rootMargin
//    '50%', threshold 0), which also *empties* every image that leaves that
//    margin, so each return costs a request, a decode and a fade-in again.
// This runs before the page scripts and wraps the constructor: observers that
// match either signature get a margin of about three screens below, one above
// and most of a screen to the sides (for row scrollers), and the in-house
// loader additionally never sees the "left the viewport" entries that make it
// unload. Every other IntersectionObserver is left untouched.
(function () {
    const Native = window.IntersectionObserver;
    if (typeof Native !== 'function' || Native.__jfdWrapped) return;

    const AHEAD_MARGIN = '100% 75% 300% 75%'; // top right bottom left

    // react-lazy-load-image-component: `{ rootMargin: '100px' }`, nothing else.
    function isPixelLazyLoader(options) {
        return !!options
            && typeof options.rootMargin === 'string'
            && /^\d+px$/.test(options.rootMargin)
            && !options.root
            && options.threshold === undefined;
    }

    // jellyfin-web's lazyLoaderIntersectionObserver: `{ rootMargin: '50%', threshold: 0 }`.
    function isUnloadingLazyLoader(options) {
        return !!options
            && options.rootMargin === '50%'
            && !options.root
            && (options.threshold === 0 || options.threshold === undefined);
    }

    function Wrapped(callback, options) {
        if (isPixelLazyLoader(options)) {
            return new Native(callback, Object.assign({}, options, { rootMargin: AHEAD_MARGIN }));
        }
        if (!isUnloadingLazyLoader(options)) {
            return new Native(callback, options);
        }
        let observer = null;
        const filtered = (entries) => {
            const kept = [];
            for (let i = 0; i < entries.length; i++) {
                const entry = entries[i];
                if (!entry.isIntersecting) continue;
                if (entry.target.hasAttribute('data-src')) {
                    kept.push(entry);
                } else {
                    // Already filled: nothing left for the page to do here.
                    observer.unobserve(entry.target);
                }
            }
            if (kept.length) callback(kept, observer);
        };
        observer = new Native(filtered, Object.assign({}, options, { rootMargin: AHEAD_MARGIN }));
        return observer;
    }
    Wrapped.prototype = Native.prototype;
    Wrapped.__jfdWrapped = true;
    window.IntersectionObserver = Wrapped;
})();
