/**
 * Polyfill for WebGL shader precision format lookup.
 *
 * In certain browser/webview environments (e.g., software renderers, SwiftShader,
 * virtualized displays, or buggy GPU drivers), `gl.getShaderPrecisionFormat(...)`
 * returns `null`.
 *
 * Three.js WebGLCapabilities internally evaluates:
 *   gl.getShaderPrecisionFormat(gl.VERTEX_SHADER, gl.HIGH_FLOAT).precision > 0
 *
 * If getShaderPrecisionFormat returns null, Three.js throws:
 *   TypeError: Cannot read properties of null (reading 'precision')
 *
 * This patch intercepts getShaderPrecisionFormat calls that return null and
 * provides a safe WebGLShaderPrecisionFormat fallback object:
 * - For HIGH_FLOAT: returns precision: 0 so Three.js cleanly falls back to mediump.
 * - For MEDIUM_FLOAT / LOW_FLOAT: returns standard valid precision values.
 */
export function installWebGLPrecisionPolyfill(): void {
  if (typeof window === 'undefined') return;

  const patchPrototype = (proto: any) => {
    if (!proto || typeof proto.getShaderPrecisionFormat !== 'function') return;
    const orig = proto.getShaderPrecisionFormat;
    if ((orig as any).__odon_patched) return;

    const patched = function (this: any, shaderType: number, precisionType: number) {
      try {
        const res = orig.call(this, shaderType, precisionType);
        if (res && typeof res.precision === 'number') {
          return res;
        }
      } catch {
        // Fall through to fallback
      }

      // gl.HIGH_FLOAT is 0x8DF2 (36338)
      const gl = this;
      const isHigh =
        (gl && typeof gl.HIGH_FLOAT === 'number' && precisionType === gl.HIGH_FLOAT) ||
        precisionType === 36338;

      if (isHigh) {
        return { rangeMin: 0, rangeMax: 0, precision: 0 };
      }
      return { rangeMin: 1, rangeMax: 1, precision: 14 };
    };

    (patched as any).__odon_patched = true;
    proto.getShaderPrecisionFormat = patched;
  };

  try {
    if (typeof WebGLRenderingContext !== 'undefined') {
      patchPrototype(WebGLRenderingContext.prototype);
    }
  } catch {}

  try {
    if (typeof WebGL2RenderingContext !== 'undefined') {
      patchPrototype(WebGL2RenderingContext.prototype);
    }
  } catch {}
}

// Auto-install on module import
installWebGLPrecisionPolyfill();
