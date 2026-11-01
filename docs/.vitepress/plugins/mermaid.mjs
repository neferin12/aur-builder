// Convert only Mermaid fences; leave VitePress syntax highlighting unchanged.
export function mermaidPlugin(md) {
  const fence = md.renderer.rules.fence
  md.renderer.rules.fence = (tokens, index, ...args) => {
    const token = tokens[index]
    if (token.info.trim() !== 'mermaid') {
      return fence(tokens, index, ...args)
    }
    // Encode the complete source so markup cannot become Vue attributes or directives.
    return `<MermaidDiagram code="${encodeURIComponent(token.content)}" />\n`
  }
}
