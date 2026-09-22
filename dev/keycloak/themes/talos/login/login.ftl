<#import "template.ftl" as layout>
<@layout.registrationLayout displayMessage=!messagesPerField.existsError('username','password') displayInfo=realm.password && realm.registrationAllowed && !registrationDisabled??; section>
    <#if section = "header">
        ${msg("loginAccountTitle")}
    <#elseif section = "form">
        <#if realm.password>
            <form id="kc-form-login" onsubmit="login.disabled = true; return true;" action="${url.loginAction}" method="post" class="talos-form">
                <#if !usernameHidden??>
                    <label class="field">
                        <span>${msg("usernameOrEmail")}</span>
                        <input tabindex="1" id="username" class="input" name="username" value="${(login.username!'')}" type="text" autofocus autocomplete="username"
                               aria-invalid="<#if messagesPerField.existsError('username','password')>true</#if>"
                               placeholder="USER / EMAIL"/>
                    </label>
                </#if>

                <label class="field">
                    <span>${msg("password")}</span>
                    <input tabindex="2" id="password" class="input" name="password" type="password" autocomplete="current-password"
                           aria-invalid="<#if messagesPerField.existsError('username','password')>true</#if>"
                           placeholder="••••••••••••"/>
                </label>

                <div class="row-opts">
                    <#if realm.rememberMe && !usernameHidden??>
                        <label class="check">
                            <input tabindex="3" id="rememberMe" name="rememberMe" type="checkbox" <#if login.rememberMe??>checked</#if>/>
                            <span>${msg("rememberMe")}</span>
                        </label>
                    <#else>
                        <span></span>
                    </#if>
                    <#if realm.resetPasswordAllowed>
                        <a tabindex="5" href="${url.loginResetCredentialsUrl}" class="linkish">${msg("doForgotPassword")}</a>
                    </#if>
                </div>

                <input type="hidden" id="id-hidden-input" name="credentialId" <#if auth.selectedCredential?has_content>value="${auth.selectedCredential}"</#if>/>
                <button tabindex="4" class="btn-primary" name="login" id="kc-login" type="submit">${msg("doLogIn")}</button>
            </form>
        </#if>
    <#elseif section = "info">
        <#if realm.password && realm.registrationAllowed && !registrationDisabled??>
            <p class="register-hint">
                ${msg("noAccount")}
                <a tabindex="6" href="${url.registrationUrl}" class="linkish">${msg("doRegister")}</a>
            </p>
        </#if>
    </#if>
</@layout.registrationLayout>
