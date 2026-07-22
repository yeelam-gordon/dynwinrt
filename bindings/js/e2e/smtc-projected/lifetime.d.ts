export declare function trackProjectedValue<T extends object>(value: T, typeName: string): T;
export interface ProjectedLifetimeScope {
readonly disposed: boolean;
dispose(): void;
}
export declare function createProjectedLifetimeScope(): ProjectedLifetimeScope;
