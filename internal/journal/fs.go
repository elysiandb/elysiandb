package journal

import "github.com/taymour/elysiandb/internal/durable"

type File = durable.File

type FS = durable.FS

var OSFS FS = durable.OS
