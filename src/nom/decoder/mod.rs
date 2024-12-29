pub mod number;

const VALID_LP_CHARS: [char; 18] = ['!', '#', '$', '%', '&', '(', ')', '_', ',', '.', ';', '?', '@', '\\', '{', '}', '~', '\''];
const SECTION_HEADERS: [&str; 12] =
    ["integers", "integer", "general", "generals", "gen", "binaries", "binary", "bin", "bounds", "bound", "sos", "end"];
