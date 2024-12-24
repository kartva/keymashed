# LLM-generated file; only for README diagrams.

import numpy as np
import matplotlib.pyplot as plt
from scipy.fft import dct, idct

# Function to apply 2D DCT
def dct2(block):
    return dct(dct(block.T, norm='ortho').T, norm='ortho')

# Function to apply 2D inverse DCT
def idct2(block):
    return idct(idct(block.T, norm='ortho').T, norm='ortho')

# Example 8x8 pixel block (values ranging from 0 to 255)
pixel_block = np.array([
    [52, 55, 61, 66, 70, 61, 64, 73],
    [63, 59, 55, 90, 109, 85, 69, 72],
    [62, 59, 68, 113, 144, 104, 66, 73],
    [63, 58, 71, 122, 154, 106, 70, 69],
    [67, 61, 68, 104, 126, 88, 68, 70],
    [79, 65, 60, 70, 77, 68, 58, 75],
    [85, 71, 64, 59, 55, 61, 65, 83],
    [87, 79, 69, 68, 65, 76, 78, 94]
])

# Zero-center the pixel block by subtracting 128
zero_centered_block = pixel_block - 128

# Quantization matrix (example from JPEG standard)
quantization_matrix = np.array([
    [16, 11, 10, 16, 24, 40, 51, 61],
    [12, 12, 14, 19, 26, 58, 60, 55],
    [14, 13, 16, 24, 40, 57, 69, 56],
    [14, 17, 22, 29, 51, 87, 80, 62],
    [18, 22, 37, 56, 68, 109, 103, 77],
    [24, 35, 55, 64, 81, 104, 113, 92],
    [49, 64, 78, 87, 103, 121, 120, 101],
    [72, 92, 95, 98, 112, 100, 103, 99]
])

# Perform DCT
dct_block = dct2(zero_centered_block)

# Quantization step
quantized_block = np.round(dct_block / quantization_matrix)
quantized_block[np.isclose(quantized_block, 0)] = 0  # Replace near-zero values with 0

# Dequantization step
dequantized_block = quantized_block * quantization_matrix

# Perform Inverse DCT
reconstructed_block = idct2(dequantized_block)

# Add 128 back to reverse zero-centering
reconstructed_block += 128

# Function to display a grid with numbers and save as SVG
def save_grid(data, title, filename):
    fig, ax = plt.subplots(figsize=(6, 6))
    ax.imshow(data, cmap='gray')
    ax.set_title(title)
    ax.axis('off')
    for (i, j), val in np.ndenumerate(data):
        ax.text(j, i, f"{val:.0f}", ha='center', va='center', color='red')
    plt.savefig(filename, format='svg', bbox_inches='tight')
    plt.close(fig)

# Save individual images
save_grid(pixel_block, "Original 8x8 Block", "original_8x8_block.svg")
save_grid(dct_block, "DCT of Block", "dct_of_block.svg")
save_grid(quantization_matrix, "Quantization Matrix", "quantization_matrix.svg")
save_grid(quantized_block, "Quantized DCT Block", "quantized_dct_block.svg")
save_grid(np.round(reconstructed_block), "Reconstructed Block", "reconstructed_block.svg")
