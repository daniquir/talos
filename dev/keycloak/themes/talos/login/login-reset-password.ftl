<#import "template.ftl" as layout>
<@layout.registrationLayout displayInfo=true displayMessage=!messagesPerField.existsError('username'); section>
    <#if section = "header">
        ${msg("emailForgotTitle")!msg("doForgotPassword")}
    <#elseif section = "form">
        <form id="kc-reset-password-form" action="${url.loginAction}" method="post" class="talos-form">
            <label class="field">
                <span>${msg("usernameOrEmail")}</span>
                <input type="text" id="username" name="username" class="input" autofocus
                       value="${(auth.attemptedUsername!'')}"
                       aria-invalid="<#if messagesPerField.existsError('username')>true</#if>"
                       placeholder="USER / EMAIL"/>
            </label>
            <button class="btn-primary" type="submit">${msg("doSubmit")}</button>
            <p class="register-hint">
                <a href="${url.loginUrl}" class="linkish">${msg("backToLogin")}</a>
            </p>
        </form>
    <#elseif section = "info">
        <p class="register-hint">${msg("emailInstruction")!""}</p>
    </#if>
</@layout.registrationLayout>
