// 由 export_render_bindings 从 Rust 自动生成，请勿手工编辑。
import type { Bounds, Capability, TextIndexMapping, TextStyleRun, ParagraphStyleRun, TextDiagnostic } from '../shared/api/generated';

export type TextData = { rawText: string, utf16Length: number, 
/**
 * 文字空间到文档空间的 [a,b,c,d,tx,ty]；未应用到字号或再次应用到边界。
 */
transform: [number, number, number, number, number, number] | null, normalizationChanged: boolean, styles: Capability, indexMapping: TextIndexMapping | null, styleRuns: Array<TextStyleRun> | null, paragraphRuns: Array<ParagraphStyleRun> | null, diagnostics: Array<TextDiagnostic>, };

export type RenderPath = { knots: Array<[number, number, number, number, number, number]>, evenOdd: boolean, };

export type RenderContent = { "kind": "bitmap", 
/**
 * Large 档位为空，调用 render_resource 读取该层；其他档位包含 PNG。
 */
png: Array<number>, } | { "kind": "text", data: TextData, textBox: RenderTextBox | null, } | { "kind": "path", paths: Array<RenderPath>, color: [number, number, number, number], } | { "kind": "group" } | { "kind": "unsupported", reason: string, };

export type RenderLayer = { id: number, parentId: number | null, name: string, bounds: Bounds, visible: boolean, opacity: number, content: RenderContent, compositionWarning: string | null, };

export type RenderProfile = "simple" | "bounded" | "large" | "largeLayers";

export type RenderTextBox = { x: number, y: number, width: number, height: number, };

export type RenderScene = { sourceSha256: string, width: number, height: number, layers: Array<RenderLayer>, referencePng: Array<number>, warnings: Array<string>, };

