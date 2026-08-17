# Tauri Frontend — Asset Scope & Image Loading for Cartridge Drives

When a Tauri app on a cartridge needs to display images or other files that
live on the cartridge drive itself, Tauri's default security policy will block
those file:// URLs. Two patterns are provided below.

---

## Pattern A — tauri.conf.json assetScope (simplest)

Add the cartridge drive root(s) to the `assetScope` array in your
`src-tauri/tauri.conf.json`.  The frontend can then use `asset://` URLs to
load files directly.

### src-tauri/tauri.conf.json (relevant snippet)

```json
{
  "tauri": {
    "security": {
      "assetScope": [
        "D:\\**",
        "E:\\**",
        "/media/**",
        "/run/media/**",
        "/mnt/**"
      ]
    }
  }
}
```

### React / JS frontend (using Tauri's convertFileSrc helper)

```jsx
import { convertFileSrc } from '@tauri-apps/api/tauri';

// cartridgePath comes from a Tauri command that returns the drive root
const imageSrc = convertFileSrc('D:\\cover.png');  // Windows
// or
const imageSrc = convertFileSrc('/media/user/MyCartridge/cover.png');  // Linux

return <img src={imageSrc} alt="Game cover" />;
```

---

## Pattern B — Base64 data URI via Rust command (no assetScope change needed)

Read the image bytes in Rust and return them as a base64-encoded data URI.
This avoids touching `assetScope` and works regardless of the drive letter.

### src-tauri/src/main.rs

```rust
use base64::{engine::general_purpose, Engine as _};
use std::path::PathBuf;
use tauri::command;

/// Reads an image from an arbitrary path and returns a base64 data URI.
/// The frontend can use this directly as an <img src="..."> value.
#[command]
fn read_image_as_data_uri(path: String) -> Result<String, String> {
    let file_path = PathBuf::from(&path);

    let bytes = std::fs::read(&file_path)
        .map_err(|e| format!("Cannot read {}: {}", path, e))?;

    // Detect MIME type from extension
    let mime = match file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "gif"          => "image/gif",
        "webp"         => "image/webp",
        _              => "image/png",
    };

    let b64 = general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:{};base64,{}", mime, b64))
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![read_image_as_data_uri])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

**Cargo.toml — add the base64 dependency:**

```toml
[dependencies]
base64 = "0.21"
```

### React / JS frontend

```jsx
import { invoke } from '@tauri-apps/api/tauri';
import { useState, useEffect } from 'react';

function CartridgeCover({ imagePath }) {
  const [src, setSrc] = useState('');

  useEffect(() => {
    invoke('read_image_as_data_uri', { path: imagePath })
      .then(setSrc)
      .catch(console.error);
  }, [imagePath]);

  return src ? <img src={src} alt="Game cover" /> : null;
}
```

---

## Which pattern to use?

| Situation | Recommended pattern |
|---|---|
| Drive letter is fixed / known | Pattern A (assetScope) |
| Drive letter varies at runtime | Pattern B (base64 command) |
| Serving many large assets | Pattern A (browser caches the asset URL) |
| Single cover image or small files | Pattern B (simpler, no config) |
