/**
 * What the browser inside a window would still do on its own — the find
 * bar, printing, saving the page, reloading, caret browsing, a link's
 * download or new window, the middle button's autoscroll — stopped before it
 * shows. The page always goes first: a key reaches every handler of the
 * page, and only when none of them took it (`defaultPrevented`) is the
 * browser's own action cancelled. Editing keys (copy, cut, paste, undo,
 * redo, select all in a field) stay with the browser: they are the text
 * fields' own.
 *
 * The app guards its own window with these (`lib/browser-guards`), and the
 * SDK guards a plugin page with the same.
 */
/** A field that takes typing: its keys are its own. */
export declare function isEditable(t: EventTarget | null): boolean;
/** Whether the browser would act on this key (and the app didn't ask for it). */
export declare function isBrowserKey(e: KeyboardEvent, dev: boolean): boolean;
/** Clicks on a link that would make the browser download it or open a window. */
export declare function isBrowserLinkClick(e: MouseEvent): boolean;
/**
 * Install the guards on `win`; returns what removes them. Not the keys on
 * macOS: there a ⌘ key the page cancels never reaches the app's menu (⌘Q,
 * ⌘W, ⌘H, the menu's ⌘K…), and WKWebView has no browser keys of its own.
 */
export declare function installGuards({ dev, mac, win }: {
    dev: boolean;
    mac: boolean;
    win: Window;
}): () => void;
