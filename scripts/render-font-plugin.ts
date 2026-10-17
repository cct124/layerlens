/** 实验构建按本地清单绑定字体；没有清单时返回空列表，不影响普通发行构建。 */
import { existsSync, readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import path from 'node:path';
import type { Plugin } from 'vite';
export function renderFontPlugin(): Plugin {
  const id = 'virtual:render-lab-fonts';
  return {
    name: 'layerlens-local-experiment-fonts',
    resolveId(source) {
      if (source === id) return '\0' + id;
    },
    load(source) {
      if (source !== '\0' + id) return;
      const root = path.resolve('.local/render-fonts');
      if (!existsSync(path.join(root, 'manifest.json'))) return 'export default []';
      const data: unknown = JSON.parse(readFileSync(path.join(root, 'manifest.json'), 'utf8'));
      if (!Array.isArray(data) || data.length > 8) throw new Error('实验字体清单最多 8 个显式字体');
      const imports: string[] = [],
        entries: string[] = [],
        names = new Set<string>();
      let totalBytes = 0;
      for (const [index, item] of data.entries()) {
        if (
          !item ||
          typeof item !== 'object' ||
          !('file' in item) ||
          typeof item.file !== 'string' ||
          !/^[A-Za-z0-9-]+\.(ttf|otf)$/.test(item.file) ||
          !('name' in item) ||
          typeof item.name !== 'string' ||
          !/^[A-Za-z0-9-]{1,96}$/.test(item.name) ||
          names.has(item.name) ||
          !('sha' in item) ||
          typeof item.sha !== 'string' ||
          !('family' in item) ||
          typeof item.family !== 'string' ||
          !/^LayerLens Local [A-Za-z0-9-]{1,96}$/.test(item.family) ||
          !('codepoints' in item) ||
          !Array.isArray(item.codepoints) ||
          item.codepoints.length > 65536 ||
          item.codepoints.some(
            (n: unknown) => typeof n !== 'number' || !Number.isInteger(n) || n < 0 || n > 0x10ffff,
          )
        )
          throw new Error('实验字体清单无效');
        const file = path.join(root, item.file),
          bytes = readFileSync(file);
        if (
          bytes.length > 32 * 1024 * 1024 ||
          (totalBytes += bytes.length) > 96 * 1024 * 1024 ||
          createHash('sha256').update(bytes).digest('hex') !== item.sha
        )
          throw new Error('本地实验字体指纹不符');
        names.add(item.name);
        imports.push(
          'import url' +
            index +
            ' from ' +
            JSON.stringify(file.replaceAll('\\', '/') + '?url') +
            ';',
        );
        entries.push('{ ...' + JSON.stringify(item) + ', url: url' + index + ' }');
      }
      return imports.join('\n') + '\nexport default [' + entries.join(',') + '];';
    },
  };
}
