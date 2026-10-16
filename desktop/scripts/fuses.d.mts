export declare const SHIPPED_FUSES: Record<string, unknown>;
export declare function applyFuses(electron: string, options?: { adHocSign?: boolean }): Promise<void>;
export declare function assertFuses(electron: string): Promise<void>;
