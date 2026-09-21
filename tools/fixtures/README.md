# GPU command fixture

`gpu-list-classic-frame.json` contains the last completed Classic frame from
the unmodified Celeste Collection 0.2.3 executable, SHA256
`0d7526c9d88fc086ea19cb934dea2b18c6f6da54e7f83c6176e1aecf6afaae09`.
The four node payloads contain 995 GP0 words and 264 complete commands.
It includes only GPU commands and palette values, not executable code or assets.

The regression verifies command boundaries, the documented combined-primitive
size limit and exact word order after regrouping. Actual runtime node counts
and framebuffer equivalence are checked by the emulator replay separately.

Sony Run-Time Library Overview 4.6, printed page 8-13 (PDF page 97):
https://psx.arthus.net/sdk/Psy-Q/DOCS/LIBOVR46.PDF
