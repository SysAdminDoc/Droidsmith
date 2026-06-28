declare module "qrcode-generator" {
  type ErrorCorrectionLevel = "L" | "M" | "Q" | "H";

  type SvgOptions = {
    cellSize?: number;
    margin?: number;
    scalable?: boolean;
  };

  type QrCode = {
    addData(data: string): void;
    make(): void;
    createSvgTag(options?: SvgOptions): string;
  };

  export default function qrcode(
    typeNumber: number,
    errorCorrectionLevel: ErrorCorrectionLevel,
  ): QrCode;
}
