# Captures and visual verification

ocs_capture operates quietly in background and overlapped window states without stealing user focus. Viewports and captured snapshots are available as MCP resources under cad://session/{session_id}/viewport.png and cad://session/{session_id}/snapshot/{hash}.png; ocs_capture accepts delivery: 'resource' to avoid large inline base64 payloads, supplies standardized spatial grounding in _spatial, supports diff: true for streaming dirty visual regions, and provides multiscale DeepZoom pyramidal tiling via tile: {level, x, y} or cad://session/{session_id}/pyramid/manifest.json and cad://session/{session_id}/tile/{level}/{x}/{y}.png. 

Use annotate: true for entity IDs and diff: true for visual changes. Unchanged diff captures omit image data. delivery: resource returns a resource_link; read its URI only when you need the image. delivery: both includes the link and inline image.
