/** 登录态：凭据是服务端下发的 HttpOnly cookie，前端只记是否被拒。 */
class Auth {
  loggedIn = $state(true);

  /** 非登录接口返回 401：未登录或已过期，切到登录页并保留当前地址。 */
  expire(): void {
    this.loggedIn = false;
  }
}

export const auth = new Auth();
