package object

import (
	"fmt"
	"os"
)

// File represents a file handle in Sflang.
// It wraps a Go *os.File value and implements the Object interface.
type File struct {
	Value *os.File
	Path  string // Store path for debugging/inspection
}

// Type returns the ObjectType for File.
func (f *File) Type() ObjectType { return FILE_OBJ }

// Inspect returns a string representation of the file handle.
func (f *File) Inspect() string {
	if f.Value == nil {
		return fmt.Sprintf("file:%s(closed)", f.Path)
	}
	return fmt.Sprintf("file:%s", f.Path)
}

// TypeCode returns the fixed numeric type code for File.
func (f *File) TypeCode() int { return TypeCodeFile }

// TypeName returns the human-readable type name.
func (f *File) TypeName() string { return "file" }

// Close closes the file. Returns nil on success, error on failure.
func (f *File) Close() error {
	if f.Value == nil {
		return nil
	}
	err := f.Value.Close()
	f.Value = nil
	return err
}

// IsOpen returns true if the file is currently open.
func (f *File) IsOpen() bool {
	return f.Value != nil
}

// Read reads up to len(b) bytes from the file.
func (f *File) Read(b []byte) (int, error) {
	if f.Value == nil {
		return 0, fmt.Errorf("file is closed")
	}
	return f.Value.Read(b)
}

// Write writes len(b) bytes to the file.
func (f *File) Write(b []byte) (int, error) {
	if f.Value == nil {
		return 0, fmt.Errorf("file is closed")
	}
	return f.Value.Write(b)
}

// OpenFile opens a file with the specified flags and mode.
// Returns a File object or an Error.
func OpenFile(path string, flag int, perm os.FileMode) Object {
	file, err := os.OpenFile(path, flag, perm)
	if err != nil {
		return NewError("failed to open file '%s': %s", path, err.Error())
	}
	return &File{Value: file, Path: path}
}