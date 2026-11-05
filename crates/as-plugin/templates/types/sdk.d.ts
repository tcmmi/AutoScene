/**
 * The SDK of plugin pages: `sdk.js` (built from `entry.ts`), which AutoScene
 * serves at `/_as/sdk.js` beside `theme.css`. A page loads it as a classic
 * script and asks `AutoScene.connect()` for its `Page`: who it is, where it
 * is open, and the calls into AutoScene its plugin may make.
 *
 * In AutoScene the page sits in a sandboxed frame. `connect()` says `ready`
 * to its parent, takes the one port the host's `init` brings, and from then
 * on speaks only over it (`bridge-types.ts`). It also guards the frame as
 * the app guards its window (`keys.ts`), and hands the keys and right-clicks
 * the page didn't take to AutoScene, so its shortcuts and menus work over a
 * page too.
 *
 * Opened outside AutoScene (a plain browser, while it is being made), a page
 * gets a `Page` over an in-memory host instead, when it passes one
 * (`mock.ts`).
 *
 * Kept apart from the app: nothing here imports React, Radix, Tauri or the
 * app's modules (`.oxlintrc.json`), since the build is a page's script.
 */
import { type FrameToHost, type HostToFrame, type PageInit, type PageLocale, type ServiceState } from "./bridge-types";
import { type MockHost } from "./mock";
import type { ActionsRun, CopyText, ExportRef, FileRef, HostMethod, Navigate, NetRequest, NetResponse, OpenExternal, OpenFile, OpenedFile, PageExport, PageExportSummary, PageProject, Permission, ProjectRef, ResolvedFile, SaveFile, SecretsGet, ServiceRequestParams, SettingValue, StateKey, StateSet, Toast } from "./protocol";
export type * from "./protocol";
export type { MockHost } from "./mock";
export type { CallFailure, PageInit, PageLocale, ServiceState, ServiceStatus } from "./bridge-types";
/** The params of a method that takes none. */
type Empty = Record<string, never>;
/** Fails to compile unless `T` types exactly the methods AutoScene has. */
type Exactly<T extends Record<HostMethod, {
    params: unknown;
    result: unknown;
}> & Record<Exclude<keyof T, HostMethod>, never>> = T;
/**
 * What each host method takes and gives. `HostMethod` comes from Rust
 * (`protocol.ts`): a method added there and missing here, or one here that
 * Rust no longer has, fails `npm run typecheck`.
 */
export type MethodTypes = Exactly<{
    "settings.get": {
        params: Empty;
        result: Record<string, SettingValue>;
    };
    "secrets.get": {
        params: SecretsGet;
        result: {
            value: string | null;
        };
    };
    "project.get": {
        params: ProjectRef;
        result: PageProject;
    };
    "exports.list": {
        params: ProjectRef;
        result: PageExportSummary[];
    };
    "exports.get": {
        params: ExportRef;
        result: PageExport;
    };
    "files.resolve": {
        params: FileRef;
        result: ResolvedFile;
    };
    "net.fetch": {
        params: NetRequest;
        result: NetResponse;
    };
    "state.get": {
        params: StateKey;
        result: {
            value: unknown;
        };
    };
    "state.set": {
        params: StateSet;
        result: null;
    };
    "state.delete": {
        params: StateKey;
        result: null;
    };
    "state.keys": {
        params: Empty;
        result: string[];
    };
    "service.request": {
        params: ServiceRequestParams;
        result: unknown;
    };
    "actions.run": {
        params: ActionsRun;
        result: {
            jobId: string | null;
        };
    };
    "ui.toast": {
        params: Toast;
        result: {
            shown: boolean;
        };
    };
    "ui.navigate": {
        params: Navigate;
        result: null;
    };
    "ui.copyText": {
        params: CopyText;
        result: null;
    };
    "ui.openExternal": {
        params: OpenExternal;
        result: {
            opened: boolean;
        };
    };
    "files.save": {
        params: SaveFile;
        result: {
            saved: boolean;
        };
    };
    "files.open": {
        params: OpenFile;
        result: OpenedFile | null;
    };
}>;
export type Params<M extends HostMethod> = MethodTypes[M]["params"];
export type Result<M extends HostMethod> = MethodTypes[M]["result"];
/** The global `AutoScene` that `sdk.js` defines. */
export interface AutoSceneSdk {
    readonly version: 1;
    connect(options?: ConnectOptions): Promise<Page>;
}
export interface ConnectOptions {
    /** Stop the browser's own keys, link clicks and menu in the page (default on). */
    guards?: boolean;
    /** Hand the keys and right-clicks the page didn't take to AutoScene (default on). */
    forwardKeys?: boolean;
    /** How long to wait for AutoScene, in ms (default 10 000). */
    timeoutMs?: number;
    /** The host to answer from when the page isn't open in AutoScene. */
    mock?: MockHost;
}
/** A plugin page, connected. */
export interface Page {
    readonly plugin: {
        id: string;
        name: string;
        version: string;
    };
    readonly page: {
        id: string;
        title: string;
        scope: "global" | "project";
    };
    /** The project it is open in; null for a page of its own. */
    readonly projectId: string | null;
    /** The path inside the page; `onRoute` says when it changes. */
    readonly path: string;
    /** What its calls may use. */
    readonly permissions: Permission[];
    /**
     * AutoScene as the page found it: its version, the platform, and the language it speaks now
     * (`"en"` or `"ro"`). A page that wants to match AutoScene's language reads it; nothing in the page
     * changes by itself. AutoScene reloads an open page when the person switches language, and the
     * next one reads the new language here.
     */
    readonly app: {
        readonly version: string;
        readonly platform: "windows" | "macos" | "linux";
        readonly locale: PageLocale;
    };
    /** Any host method, as it is: the helpers below are made of it. */
    call<M extends HostMethod>(method: M, params: Params<M>): Promise<Result<M>>;
    project: {
        get(): Promise<PageProject>;
    };
    exports: {
        list(): Promise<PageExportSummary[]>;
        get(exportId: string): Promise<PageExport>;
    };
    files: {
        resolve(ref: FileRef): Promise<ResolvedFile>;
        save(f: SaveFile): Promise<boolean>;
        open(f?: OpenFile): Promise<OpenedFile | null>;
    };
    net: {
        fetch(r: NetRequest): Promise<NetResponse>;
    };
    secrets: {
        get(key: string): Promise<string | null>;
    };
    settings: {
        get(): Promise<Record<string, SettingValue>>;
        onChange(cb: () => void): () => void;
    };
    state: {
        get(key: string): Promise<unknown>;
        set(key: string, value: unknown): Promise<void>;
        delete(key: string): Promise<void>;
        keys(): Promise<string[]>;
    };
    service: {
        request(method: string, params?: unknown, o?: {
            timeoutMs?: number;
        }): Promise<unknown>;
        on(event: string, cb: (data: unknown) => void): () => void;
        /** Its service's state as the page knows it now, if it knows one, then each change. */
        onState(cb: (s: ServiceState) => void): () => void;
    };
    actions: {
        run(action: string, o?: {
            exportId?: string;
            input?: unknown;
        }): Promise<string | null>;
    };
    ui: {
        toast(text: string, o?: {
            description?: string;
            tone?: "neutral" | "error";
        }): Promise<void>;
        navigate(path: string, o?: {
            replace?: boolean;
        }): Promise<void>;
        copyText(text: string): Promise<void>;
        openExternal(url: string): Promise<boolean>;
    };
    onRoute(cb: (path: string) => void): () => void;
    /**
     * A host told the open page that AutoScene's language changed: each change once, never the language it
     * has now, and `app.locale` is the new one when `cb` runs. AutoScene itself reloads the page on a switch
     * instead, so this fires only under a host that keeps pages open.
     */
    onLocaleChange(cb: (locale: PageLocale) => void): () => void;
    onClose(cb: (reason: string) => void): () => void;
}
/**
 * The host as a page sees it, over the bridge or from the mock: what the
 * page was told, its calls, and what the host says unasked.
 */
export interface HostLink {
    readonly init: PageInit;
    call(method: HostMethod, params: unknown): Promise<unknown>;
    send(m: FrameToHost): void;
    listen(cb: (m: HostToFrame) => void): void;
}
export declare const version = 1;
/** Connect the page to AutoScene (or to `options.mock` outside it). */
export declare function connect(options?: ConnectOptions): Promise<Page>;
