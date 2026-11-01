package recovery

import (
	"bufio"
	"bytes"
	"io"
	"os"
)

const FailedReplaySuffix = ".failed"

func replayLines(f *os.File, apply func(line []byte)) error {
	reader := bufio.NewReader(replayReader(f))
	for {
		line, err := reader.ReadBytes('\n')
		if trimmed := bytes.TrimSpace(line); len(trimmed) > 0 {
			apply(trimmed)
		}

		if err == io.EOF {
			return nil
		}

		if err != nil {
			return err
		}
	}
}

func replayReader(f *os.File) io.Reader {
	info, err := f.Stat()
	if err != nil {
		return f
	}

	return io.LimitReader(f, info.Size())
}

func keepUnreplayedLog(path string, err error) {
	failedPath := path + FailedReplaySuffix
	if renameErr := os.Rename(path, failedPath); renameErr != nil {
		recoveryLog.Error("cannot read the recovery log, and it could not be kept", "file", path, "err", err, "rename_err", renameErr)
		return
	}

	recoveryLog.Error("cannot read the recovery log, it was kept aside", "file", path, "kept_as", failedPath, "err", err)
}
