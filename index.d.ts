/** Resampling filters. Values match the addon this replaces. */
export const enum FastResizeFilter {
    Box = 0,
    Bilinear = 1,
    Hamming = 2,
    CatmullRom = 3,
    Mitchell = 4,
    Lanczos3 = 5,
}

/** PNG compression presets. */
export const enum CompressionType {
    Default = 0,
    Fast = 1,
    Best = 2,
}

export interface FastResizeOptions {
    width: number;
    height?: number;
    filter?: FastResizeFilter;
}

export interface PngEncodeOptions {
    compressionType?: CompressionType;
}

export interface Metadata {
    width: number;
    height: number;
    format: string;
}

export class Transformer {
    constructor(input: Buffer);
    /** Dimensions and container from the header. No decode, no EXIF. */
    metadata(withExif?: boolean): Promise<Metadata>;
    /** Queue a resize; the work happens when an encoder is called. */
    fastResize(options: FastResizeOptions): void;
    jpeg(quality?: number): Promise<Buffer>;
    png(options?: PngEncodeOptions): Promise<Buffer>;
    webp(quality?: number): Promise<Buffer>;
}
