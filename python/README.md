# parse_lp

A fast LP file format parser for Python, powered by Rust.

## Installation

```bash
pip install parse_lp
```

## Usage

```python
from parse_lp import LpParser

# Parse an LP file
parser = LpParser("path/to/file.lp")

# Export to CSV files
parser.to_csv("output_directory/")
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.