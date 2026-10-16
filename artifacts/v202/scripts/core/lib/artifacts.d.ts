import { type Profile, type Module, type Frame } from './state.js';
export { COMPILER_COMMIT } from './state.js';
/** Structural parser only; ABI/hash validity is NOT approval for the KW21 codec. */
export declare function parseCompiledFrame(module: Module, doc: unknown, sourceSha256: string): Frame;
export declare function makeProfile(frames: Record<Module, Frame>): Profile;
export declare function loadTrustedProfile(inputs: Record<Module, {
    text: string;
    expectedArtifactSha256: string;
    sourceSha256: string;
}>): Promise<Profile>;
