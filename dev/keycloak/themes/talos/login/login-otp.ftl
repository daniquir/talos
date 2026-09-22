<#import "template.ftl" as layout>
<@layout.registrationLayout; section>
    <#if section = "header">
        ${msg("loginTotpTitle")}
    <#elseif section = "form">
        <form id="kc-otp-login-form" action="${url.loginAction}" method="post" class="talos-form">
            <label class="field">
                <span>${msg("loginTotpOneTime")}</span>
                <input id="otp" name="otp" autocomplete="one-time-code" type="text" class="input" autofocus placeholder="000000"/>
            </label>
            <button class="btn-primary" name="login" type="submit">${msg("doLogIn")}</button>
        </form>
    </#if>
</@layout.registrationLayout>
