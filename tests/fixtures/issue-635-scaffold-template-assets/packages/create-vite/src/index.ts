import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export function scaffold(template: string, root: string) {
  const templateDir = path.resolve(
    fileURLToPath(import.meta.url),
    '../..',
    `template-${template}`,
  )

  for (const file of fs.readdirSync(templateDir)) {
    copy(path.join(templateDir, file), path.join(root, file))
  }
}

function copy(src: string, dest: string) {
  const stat = fs.statSync(src)
  if (stat.isDirectory()) {
    fs.mkdirSync(dest, { recursive: true })
    for (const file of fs.readdirSync(src)) {
      copy(path.join(src, file), path.join(dest, file))
    }
    return
  }

  fs.copyFileSync(src, dest)
}
