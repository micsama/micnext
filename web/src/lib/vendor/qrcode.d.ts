// qrcode 1.5.4 的浏览器入口；bundle 含 dijkstrajs 1.0.3，许可证见同目录。
declare const QRCode: {
  toDataURL(text: string, options: { width: number; margin: number; errorCorrectionLevel: "M" }): Promise<string>;
};
export default QRCode;
