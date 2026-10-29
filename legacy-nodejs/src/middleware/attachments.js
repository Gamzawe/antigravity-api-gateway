const attachmentTypes = new Set(['file', 'document', 'input_file', 'image', 'image_url', 'input_image']);

/** Inspect structured attachments only: prose about file: URLs remains valid. */
export function rejectLocalAttachments(req, res, next) {
  const pending = [req.body];
  while (pending.length) {
    const item = pending.pop();
    if (!item || typeof item !== 'object') continue;
    if (attachmentTypes.has(item.type)) {
      const urls = [item.url, item.file?.url, typeof item.file_url === 'string' ? item.file_url : item.file_url?.url,
        item.source?.url, typeof item.image_url === 'string' ? item.image_url : item.image_url?.url];
      if (urls.some(url => typeof url === 'string' && url && !/^(https?:\/\/|data:)/i.test(url))) {
        return res.status(400).json({ error: { type: 'invalid_request_error', message: 'Local file URLs and paths are not allowed. Send inline base64 instead.' } });
      }
    }
    for (const value of Object.values(item)) if (value && typeof value === 'object') pending.push(value);
  }
  next();
}
