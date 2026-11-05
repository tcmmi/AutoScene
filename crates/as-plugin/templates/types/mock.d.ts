/**
 * An in-memory host for a page opened outside AutoScene: in a plain browser,
 * while it is being made. `connect({ mock })` answers the page from here as
 * AutoScene would — the same results, and the same refusals for what its
 * plugin may not use — from the data the page passes. What only AutoScene
 * has (files, the network, a service, runs) answers through `answer`, or
 * says the mock doesn't have it.
 */
import { type PageLocale } from "./bridge-types";
import type { HostMethod, PageExport, PageProject, PageScope, Permission, SettingValue } from "./protocol";
import type { HostLink, Params, Result } from "./sdk";
export interface MockHost {
    plugin?: {
        id: string;
        name: string;
        version: string;
    };
    page?: {
        id: string;
        title: string;
        scope: PageScope;
    };
    /** The project the page is open in. */
    projectId?: string | null;
    path?: string;
    /** What its calls may use, as if the person agreed to it. */
    permissions?: Permission[];
    /** Where `net.fetch` may connect. */
    hosts?: string[];
    settings?: Record<string, SettingValue>;
    /** Its secret settings by key; null for one the person hasn't set. */
    secrets?: Record<string, string | null>;
    project?: PageProject;
    exports?: PageExport[];
    /** Its plugin runs a service. */
    service?: boolean;
    /** The language AutoScene speaks (`"en"` when left out). The mock never changes it, so a page's `onLocaleChange` never fires here: AutoScene reloads an open page on a switch, and the page reads the new language when it connects. */
    locale?: PageLocale;
    /** Answers for any method, in place of the mock's own. */
    answer?: {
        [M in HostMethod]?: (params: Params<M>) => Result<M> | Promise<Result<M>>;
    };
}
/** The mock's side of the bridge. */
export declare function createMock(host: MockHost, win: Window): HostLink;
