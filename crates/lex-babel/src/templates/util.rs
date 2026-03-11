use pathdiff::diff_paths;
use std::path::Path;

pub(crate) fn normalize_path(path: &Path, document_dir: Option<&Path>) -> String {
    let candidate = if let Some(base) = document_dir {
        diff_paths(path, base).unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    };

    let converted = to_forward_slashes(&candidate);
    if converted.starts_with("./")
        || converted.starts_with("../")
        || converted.starts_with('/')
        || converted.contains(':')
    {
        converted
    } else {
        format!("./{converted}")
    }
}

fn to_forward_slashes(path: &Path) -> String {
    path.to_string_lossy().replace("\\", "/")
}
