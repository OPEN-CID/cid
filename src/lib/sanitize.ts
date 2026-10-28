import DOMPurify from "dompurify";

/**
 * Sanitize third-party HTML (MCP App content, artifact bodies) before it
 * touches `dangerouslySetInnerHTML`.
 *
 * DOMPurify's defaults already strip scripts and event handlers; this profile
 * additionally forbids form elements (MCP `form` payloads are rendered as real
 * React controls by `McpAppCard` — raw HTML forms posting outward would be an
 * exfil channel) and forces links outward (`target=_blank` + `noopener`, so
 * rendered content can never navigate the app itself away).
 *
 * What this is NOT: a boundary against a malicious renderer — fully
 * interactive agent artifacts run in a sandboxed iframe (`ArtifactCard`),
 * not through here. This is the inline-content path only.
 */
export function sanitizeThirdPartyHtml(dirty: string): string {
  return DOMPurify.sanitize(dirty, {
    FORBID_TAGS: ["form", "input", "button", "select", "textarea", "object", "embed", "base"],
    FORBID_ATTR: ["formaction", "xlink:href"],
    ADD_ATTR: ["target"],
  });
}

/** Force every link in already-sanitized HTML outward with no opener access. */
export function externalizeLinks(sanitizedHtml: string): string {
  if (typeof document === "undefined") return sanitizedHtml;
  const template = document.createElement("template");
  template.innerHTML = sanitizedHtml;
  for (const anchor of template.content.querySelectorAll("a[href]")) {
    anchor.setAttribute("target", "_blank");
    anchor.setAttribute("rel", "noopener noreferrer");
  }
  return template.innerHTML;
}

/** The single entry point: sanitize, then pin links outward. */
export function sanitizeInlineHtml(dirty: string): string {
  return externalizeLinks(sanitizeThirdPartyHtml(dirty));
}
