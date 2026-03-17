import struct
import numpy as np

if __name__ == "__main__":
    rows_a, cols_a = 16, 32
    rows_b, cols_b = 32, 16

    matrix_a = np.array([[(r + i) / 50 for i in range(cols_a)] for r in range(rows_a)], dtype=np.float32)
    matrix_b = np.array([[(r + i + 2) / 50 for i in range(cols_b)] for r in range(rows_b)], dtype=np.float32)
    
    result = np.matmul(matrix_a, matrix_b)
    print(f"\nMatrix A shape: {matrix_a.shape}")
    print(f"Matrix B shape: {matrix_b.shape}")
    print("Matrix a: ", matrix_a)
    print("Matrix b: ", matrix_b)
    print(f"Result shape: {result.shape}")
    print(f"Result:\n{result}")
    
    # Emit little-endian float32 to match Rust f32::from_le_bytes parsing.
    packed_a = matrix_a.astype('<f4', copy=False).tobytes(order='C')
    packed_b = matrix_b.astype('<f4', copy=False).tobytes(order='C')
    
    with open('files/a_test.bin', 'wb') as f:
        f.write(packed_a)
    
    with open('files/b_test.bin', 'wb') as f:
        f.write(packed_b)
    
    print(f"\nMatrix A saved to files/a_test.bin")
    print(f"Matrix B saved to files/b_test.bin")
